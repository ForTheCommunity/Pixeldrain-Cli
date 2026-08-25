use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::{
    album::AlbumAction,
    auth,
    progress_bar::{ProgressReader, UploadProgress},
    state::UploadState,
};

const SMALL_FILE_THRESHOLD: u64 = 20 * 1024 * 1024;

pub async fn upload(
    paths: &[PathBuf],
    album: Option<&str>,
    album_id: Option<&str>,
    formats: Option<&[String]>,
    delete: bool,
    state_path: &Option<PathBuf>,
) -> Result<()> {
    let api_key = auth::get_api_key()?;

    let files = collect_files(paths, formats)?;

    if files.is_empty() {
        println!("  No matching files found.");
        return Ok(());
    }

    if album.is_some() && album_id.is_some() {
        bail!("Cannot use --album and --album-id together");
    }

    // Load state.
    let initial_state = if let Some(path) = state_path {
        if path.exists() {
            println!("  Loading upload state: {}", path.display());
            UploadState::load(path).await?
        } else {
            UploadState::default()
        }
    } else {
        UploadState::default()
    };

    let upload_state = Arc::new(Mutex::new(initial_state));

    // Determine the album to use.
    let saved_album_id = {
        let state = upload_state.lock().await;
        state.get_album_id().map(str::to_owned)
    };

    if let (Some(saved), Some(cli_album_id)) = (&saved_album_id, album_id)
        && saved != cli_album_id
    {
        bail!(
            "State file already belongs to album '{}', \
             but --album-id '{}' was provided",
            saved,
            cli_album_id
        );
    }

    let existing_album_id = saved_album_id.or_else(|| album_id.map(str::to_owned));

    if let Some(ref id) = existing_album_id {
        println!("  Using album: {}", id);
    }

    // ---------------------------------------------------------------
    // Classify files.
    //
    // Files already present in state are removed from the upload
    // queue completely.
    //
    // They will NOT:
    //
    //   - create a progress bar
    //   - increase total upload bytes
    //   - increase the upload file counter
    //   - appear as completed uploads
    //
    // Their IDs are still retained in all_file_ids so they can be
    // included if a new album needs to be created.
    // ---------------------------------------------------------------

    let mut small_files: Vec<(PathBuf, u64)> = Vec::new();
    let mut large_files: Vec<(PathBuf, u64)> = Vec::new();

    let mut all_file_ids: Vec<String> = Vec::new();

    let mut skipped_files = 0usize;

    for file in files {
        let metadata = match tokio::fs::metadata(&file).await {
            Ok(metadata) => metadata,

            Err(e) => {
                eprintln!(
                    "  ⚠ Failed to read metadata for {}: {}",
                    file.display(),
                    e
                );

                continue;
            }
        };

        let file_size = metadata.len();

        let saved_file_id = {
            let state = upload_state.lock().await;

            state.get_file_id(&file, file_size).map(str::to_owned)
        };

        if let Some(file_id) = saved_file_id {
            skipped_files += 1;

            all_file_ids.push(file_id);

            continue;
        }

        if file_size < SMALL_FILE_THRESHOLD {
            small_files.push((file, file_size));
        } else {
            large_files.push((file, file_size));
        }
    }

    let upload_file_count = small_files.len() + large_files.len();

    if skipped_files > 0 {
        println!("  ↻ {} file(s) already uploaded, skipping.", skipped_files);
    }

    // ---------------------------------------------------------------
    // Nothing needs to be uploaded.
    //
    // We still continue to album handling because:
    //
    //   --album-id
    //   --album
    //
    // may still need to operate on the existing state.
    // ---------------------------------------------------------------

    if upload_file_count == 0 {
        println!("  ✓ Nothing new to upload.");

        if let Some(existing_album_id) = existing_album_id {
            println!("  ✓ No new files to add to album '{}'.", existing_album_id);
            return Ok(());
        }

        if album.is_none() {
            return Ok(());
        }
    }

    println!("   ✦ Files to upload : {}", upload_file_count);

    let total_bytes: u64 = small_files
        .iter()
        .map(|(_, size)| *size)
        .sum::<u64>()
        + large_files.iter().map(|(_, size)| *size).sum::<u64>();

    if upload_file_count > 0 {
        println!("   ✦ Upload size     : {}", format_bytes(total_bytes));
    }

    let http_client = Client::new();

    let progress_bar = UploadProgress::new(upload_file_count, total_bytes);

    // Files uploaded during THIS invocation.
    //
    // These are the only files that should be added to an existing
    // album.
    let new_file_ids: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    // ---------------------------------------------------------------
    // Small files.
    // ---------------------------------------------------------------

    if !small_files.is_empty() {
        let mut tasks = tokio::task::JoinSet::new();

        for (file, file_size) in small_files {
            let client = http_client.clone();
            let api_key = api_key.clone();
            let progress_bar = progress_bar.clone();
            let new_file_ids = Arc::clone(&new_file_ids);
            let upload_state = Arc::clone(&upload_state);
            let state_path = state_path.clone();

            tasks.spawn(async move {
                let filename = filename(&file);

                let file_pb = progress_bar.file_started(filename, file_size);

                let bytes = match tokio::fs::read(&file).await {
                    Ok(bytes) => bytes,

                    Err(e) => {
                        progress_bar.file_finished(&file_pb, filename, false);

                        progress_bar
                            .overall_pb
                            .println(format!("  ⚠ Failed to read {}: {}", filename, e));

                        return;
                    }
                };

                file_pb.set_position(file_size);

                if !progress_bar.is_single_file() {
                    progress_bar.overall_pb.inc(file_size);
                }

                let part = reqwest::multipart::Part::bytes(bytes)
                    .file_name(filename.to_string());

                let form = reqwest::multipart::Form::new()
                    .part("file", part);

                let response = match client
                    .post("https://pixeldrain.com/api/file")
                    .basic_auth("", Some(&api_key))
                    .multipart(form)
                    .send()
                    .await
                {
                    Ok(response) => response,

                    Err(e) => {
                        progress_bar.file_finished(&file_pb, filename, false);

                        progress_bar
                            .overall_pb
                            .println(format!(
                                "  ⚠ Upload request failed for {}: {}",
                                filename, e
                            ));

                        return;
                    }
                };

                if !response.status().is_success() {
                    let status = response.status();
                    let body = response.text().await.unwrap_or_default();

                    progress_bar.file_finished(&file_pb, filename, false);

                    progress_bar.overall_pb.println(format!(
                        "  ⚠ Upload failed for {}: HTTP {}: {}",
                        filename, status, body
                    ));

                    return;
                }

                let upload_response = match response.json::<UploadResponse>().await {
                    Ok(response) => response,

                    Err(e) => {
                        progress_bar.file_finished(&file_pb, filename, false);

                        progress_bar.overall_pb.println(format!(
                            "  ⚠ Invalid upload response for {}: {}",
                            filename, e
                        ));

                        return;
                    }
                };

                let file_id = upload_response.id;

                // This file was uploaded during this invocation.
                {
                    let mut ids = new_file_ids.lock().await;

                    if !ids.iter().any(|id| id == &file_id) {
                        ids.push(file_id.clone());
                    }
                }

                // Save upload state immediately.
                {
                    let mut state = upload_state.lock().await;

                    state.add_file(
                        file.clone(),
                        file_id,
                        file_size,
                    );

                    if let Some(path) = state_path.as_deref()
                        && let Err(e) = state.save(path).await
                    {
                        progress_bar
                            .overall_pb
                            .println(format!(
                                "  ⚠ Failed to save state: {}",
                                e
                            ));
                    }
                }

                progress_bar.file_finished(&file_pb, filename, true);

                if delete
                    && let Err(e) = tokio::fs::remove_file(&file).await
                {
                    progress_bar.overall_pb.println(format!(
                        "  ⚠ Failed to delete {}: {}",
                        filename, e
                    ));
                }
            });
        }

        while let Some(result) = tasks.join_next().await {
            if let Err(e) = result {
                eprintln!("  ⚠ Upload task failed: {}", e);
            }
        }
    }

    // ---------------------------------------------------------------
    // Large files.
    // ---------------------------------------------------------------

    for (file, file_size) in large_files {
        let filename = filename(&file);

        let file_pb = progress_bar.file_started(filename, file_size);

        let file_handle = match tokio::fs::File::open(&file).await {
            Ok(file_handle) => file_handle,

            Err(e) => {
                progress_bar.file_finished(&file_pb, filename, false);

                progress_bar
                    .overall_pb
                    .println(format!(
                        "  ⚠ Failed to open {}: {}",
                        filename, e
                    ));

                continue;
            }
        };

        let progress_reader = ProgressReader::new(
            file_handle,
            file_pb.clone(),
            progress_bar.overall_pb.clone(),
            !progress_bar.is_single_file(),
        );

        let stream = tokio_util::io::ReaderStream::new(progress_reader);

        let body = reqwest::Body::wrap_stream(stream);

        let part = reqwest::multipart::Part::stream_with_length(
            body,
            file_size,
        )
        .file_name(filename.to_string());

        let form = reqwest::multipart::Form::new()
            .part("file", part);

        let response = match http_client
            .post("https://pixeldrain.com/api/file")
            .basic_auth("", Some(&api_key))
            .multipart(form)
            .send()
            .await
        {
            Ok(response) => response,

            Err(e) => {
                progress_bar.file_finished(&file_pb, filename, false);

                progress_bar
                    .overall_pb
                    .println(format!(
                        "  ⚠ Failed to upload {}: {}",
                        filename, e
                    ));

                continue;
            }
        };

        let status = response.status();

        let body = match response.text().await {
            Ok(body) => body,

            Err(e) => {
                progress_bar.file_finished(&file_pb, filename, false);

                progress_bar
                    .overall_pb
                    .println(format!(
                        "  ⚠ Failed to read response for {}: {}",
                        filename, e
                    ));

                continue;
            }
        };

        if !status.is_success() {
            progress_bar.file_finished(&file_pb, filename, false);

            progress_bar.overall_pb.println(format!(
                "  ⚠ Upload failed for {}: HTTP {}: {}",
                filename, status, body
            ));

            continue;
        }

        let upload_response =
            match serde_json::from_str::<UploadResponse>(&body) {
                Ok(response) => response,

                Err(e) => {
                    progress_bar.file_finished(&file_pb, filename, false);

                    progress_bar.overall_pb.println(format!(
                        "  ⚠ Invalid upload response for {}: {}",
                        filename, e
                    ));

                    continue;
                }
            };

        let file_id = upload_response.id;

        // This file was uploaded during this invocation.
        {
            let mut ids = new_file_ids.lock().await;

            if !ids.iter().any(|id| id == &file_id) {
                ids.push(file_id.clone());
            }
        }

        // Save upload state immediately.
        {
            let mut state = upload_state.lock().await;

            state.add_file(
                file.clone(),
                file_id,
                file_size,
            );

            if let Some(path) = state_path.as_deref()
                && let Err(e) = state.save(path).await
            {
                progress_bar
                    .overall_pb
                    .println(format!(
                        "  ⚠ Failed to save state: {}",
                        e
                    ));
            }
        }

        progress_bar.file_finished(&file_pb, filename, true);

        if delete
            && let Err(e) = tokio::fs::remove_file(&file).await
        {
            progress_bar.overall_pb.println(format!(
                "  ⚠ Failed to delete {}: {}",
                filename, e
            ));
        }
    }

    progress_bar.all_finished();

    // ---------------------------------------------------------------
    // Album handling.
    // ---------------------------------------------------------------

    let new_file_ids = {
        let ids = new_file_ids.lock().await;
        ids.clone()
    };

    // Existing album.
    //
    // Only files uploaded during this invocation are added.
    if let Some(existing_album_id) = existing_album_id {
        if new_file_ids.is_empty() {
            println!(
                "  ✓ No new files to add to album '{}'.",
                existing_album_id
            );
        } else {
            println!(
                "\nAdding {} new file(s) to album '{}'...",
                new_file_ids.len(),
                existing_album_id
            );

            match AlbumAction::add_files_to_album(
                &http_client,
                &api_key,
                &existing_album_id,
                &new_file_ids,
            )
            .await
            {
                Ok(()) => {
                    println!("  ✓ Album updated successfully.");
                }

                Err(e) => {
                    eprintln!(
                        "  ⚠ Failed to update album '{}': {}",
                        existing_album_id, e
                    );
                }
            }
        }

        return Ok(());
    }

    // ---------------------------------------------------------------
    // New album.
    //
    // If --album was supplied and there was no existing album,
    // create the album from every file known to state.
    // ---------------------------------------------------------------

    if let Some(album_name) = album {
        let all_file_ids = {
            let state = upload_state.lock().await;

            state
                .files
                .values()
                .map(|file| file.id.clone())
                .collect::<Vec<_>>()
        };

        if all_file_ids.is_empty() {
            eprintln!("  ⚠ No files available for album creation.");
            return Ok(());
        }

        println!(
            "\nCreating album '{}' with {} file(s)...",
            album_name,
            all_file_ids.len()
        );

        match create_album(
            &http_client,
            &api_key,
            album_name,
            &all_file_ids,
        )
        .await
        {
            Ok(new_album_id) => {
                println!("✓ Album created successfully!");
                println!(
                    "  https://pixeldrain.com/l/{}",
                    new_album_id
                );

                if let Some(path) = state_path.as_deref() {
                    let mut state = upload_state.lock().await;

                    state.set_album_id(new_album_id);

                    if let Err(e) = state.save(path).await {
                        eprintln!(
                            "  ⚠ Failed to save album ID to state: {}",
                            e
                        );
                    } else {
                        println!("  ✓ Album ID saved to state.");
                    }
                }
            }

            Err(e) => {
                eprintln!(
                    "  ⚠ Failed to create album '{}': {}",
                    album_name, e
                );
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------

fn filename(path: &Path) -> &str {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file")
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &[
        "B",
        "KiB",
        "MiB",
        "GiB",
        "TiB",
    ];

    let mut value = bytes as f64;
    let mut unit = 0usize;

    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{:.2} {}", value, UNITS[unit])
    }
}

// ---------------------------------------------------------------
// Files collection
// ---------------------------------------------------------------

fn collect_files(
    paths: &[PathBuf],
    formats: Option<&[String]>,
) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for path in paths {
        if !path.exists() {
            bail!("Path Doesn't Exists !!! {}", path.display());
        }

        if path.is_file() {
            if matches_format(path, formats) {
                files.push(path.clone());
            }
        } else if path.is_dir() {
            collect_files_from_dir(
                path,
                formats,
                &mut files,
            )?;
        } else {
            bail!(
                "Path is neither a file nor a directory: {}",
                path.display()
            );
        }
    }

    // Natural ordering.
    files.sort_by(|a, b| {
        natord::compare(
            &a.to_string_lossy(),
            &b.to_string_lossy(),
        )
    });

    // Remove duplicates.
    files.dedup();

    Ok(files)
}

fn collect_files_from_dir(
    directory: &Path,
    formats: Option<&[String]>,
    files: &mut Vec<PathBuf>,
) -> Result<()> {
    let dir_entries = std::fs::read_dir(directory)
        .with_context(|| {
            format!(
                "Failed to read directory: {}",
                directory.display()
            )
        })?;

    for entry in dir_entries {
        let dir_entry = entry?;
        let path = dir_entry.path();

        if path.is_file() {
            if matches_format(&path, formats) {
                files.push(path);
            }
        } else if path.is_dir() {
            collect_files_from_dir(
                &path,
                formats,
                files,
            )?;
        }
    }

    Ok(())
}

fn matches_format(
    path: &Path,
    formats: Option<&[String]>,
) -> bool {
    let Some(formats) = formats else {
        return true;
    };

    let Some(extension) =
        path.extension().and_then(|e| e.to_str())
    else {
        return false;
    };

    let extension = extension.to_ascii_lowercase();

    formats.iter().any(|format| {
        format
            .trim_start_matches('.')
            .eq_ignore_ascii_case(&extension)
    })
}

// ---------------------------------------------------------------
// Album creation
// ---------------------------------------------------------------

async fn create_album(
    http_client: &Client,
    api_key: &str,
    album_name: &str,
    file_ids: &[String],
) -> Result<String> {
    if file_ids.is_empty() {
        bail!("Cannot create an album without files");
    }

    let files = file_ids
        .iter()
        .map(|id| {
            serde_json::json!({
                "id": id
            })
        })
        .collect::<Vec<_>>();

    let payload = serde_json::json!({
        "title": album_name,
        "files": files,
    });

    let response = http_client
        .post("https://pixeldrain.com/api/list")
        .basic_auth("", Some(api_key))
        .json(&payload)
        .send()
        .await
        .context("Failed to send album creation request")?;

    let status = response.status();

    let body = response
        .text()
        .await
        .context("Failed to read album creation response")?;

    if !status.is_success() {
        bail!(
            "Failed to create album: HTTP {}\n{}",
            status,
            body
        );
    }

    let response_json: serde_json::Value =
        serde_json::from_str(&body)
            .context("Invalid album creation response")?;

    let album_id = response_json
        .get("id")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Album response did not contain an ID"
            )
        })?;

    Ok(album_id.to_owned())
}

#[derive(Debug, Deserialize)]
struct UploadResponse {
    id: String,
}
