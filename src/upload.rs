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

    // --album and --album-id are mutually exclusive.
    if album.is_some() && album_id.is_some() {
        bail!("Cannot use --album and --album-id together");
    }


    // Load upload state
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


    // Determine existing album
  
    let saved_album_id = {
        let state = upload_state.lock().await;
        state.get_album_id().map(str::to_owned)
    };

    // If both state and CLI specify an album ID, they must match.
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

    // State album ID takes precedence over --album-id.
    let existing_album_id = saved_album_id.or_else(|| album_id.map(str::to_owned));

    if let Some(ref id) = existing_album_id {
        println!("  Using album: {}", id);
    }


    // File classification
    const SMALL_FILE_THRESHOLD: u64 = 20 * 1024 * 1024;

    let mut small_files: Vec<(PathBuf, u64)> = Vec::new();
    let mut large_files: Vec<(PathBuf, u64)> = Vec::new();

    for file in files {
        match tokio::fs::metadata(&file).await {
            Ok(metadata) => {
                let size = metadata.len();

                if size < SMALL_FILE_THRESHOLD {
                    small_files.push((file, size));
                } else {
                    large_files.push((file, size));
                }
            }

            Err(e) => {
                eprintln!("  ⚠ Failed to read metadata for {}: {}", file.display(), e);
            }
        }
    }

    let total_files = small_files.len() + large_files.len();

    if total_files == 0 {
        println!("  No readable files found.");
        return Ok(());
    }

    println!("   ✦ Total Files : {}", total_files);

    let http_c = Client::new();

    let total_bytes: u64 = small_files.iter().map(|(_, size)| *size).sum::<u64>()
        + large_files.iter().map(|(_, size)| *size).sum::<u64>();

    let progress_bar = UploadProgress::new(total_files, total_bytes);

    // ================================================================
    // File ID tracking
    // ================================================================
    //
    // `file_ids`
    //     IDs of files already in state OR uploaded in this invocation.
    //
    //     Used when creating a NEW album.
    //
    // `new_file_ids`
    //     IDs uploaded during THIS invocation only.
    //
    //     Used when adding files to an EXISTING album.
    //
    // This distinction prevents us from repeatedly adding old state
    // files to an existing Pixeldrain album.
    // ================================================================

    let file_ids: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let new_file_ids: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

 
    // Small files
    if !small_files.is_empty() {
        let mut set = tokio::task::JoinSet::new();

        for (a_file, file_size) in small_files {
            // Check state
            let saved_file_id = {
                let state = upload_state.lock().await;

                state.get_file_id(&a_file, file_size).map(str::to_owned)
            };

            if let Some(file_id) = saved_file_id {
                let filename = a_file
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("file");

                println!("  ↻ [State File] Already uploaded, skipping: {}", filename);

                // Mark skipped file as completed.
                let file_pb = progress_bar.file_started(filename, file_size);

                file_pb.set_position(file_size);

                if !progress_bar.is_single_file() {
                    progress_bar.overall_pb.inc(file_size);
                }

                progress_bar.file_finished(&file_pb, filename, true);

                // This file can be used if we need to create a new album.
                file_ids.lock().await.push(file_id);

                // IMPORTANT:
                //
                // Do NOT add it to new_file_ids.
                //
                // It was uploaded during a previous invocation.
                continue;
            }

      
            // Upload task
            let client = http_c.clone();
            let api_key = api_key.clone();
            let progress_bar = progress_bar.clone();
            let file_ids = Arc::clone(&file_ids);
            let new_file_ids = Arc::clone(&new_file_ids);
            let upload_state = Arc::clone(&upload_state);
            let state_path = state_path.clone();

            set.spawn(async move {
                let filename = a_file
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("file");

                let file_pb = progress_bar.file_started(filename, file_size);

                // Reading file
                let bytes = match tokio::fs::read(&a_file).await {
                    Ok(bytes) => bytes,

                    Err(e) => {
                        progress_bar.file_finished(&file_pb, filename, false);

                        progress_bar
                            .overall_pb
                            .println(format!("  ⚠ Failed to read {}: {}", filename, e));

                        return;
                    }
                };

                // Small file is already fully read.
                file_pb.set_position(file_size);

                if !progress_bar.is_single_file() {
                    progress_bar.overall_pb.inc(file_size);
                }

                let part = reqwest::multipart::Part::bytes(bytes).file_name(filename.to_string());

                let form = reqwest::multipart::Form::new().part("file", part);

                // Upload
                let response = client
                    .post("https://pixeldrain.com/api/file")
                    .basic_auth("", Some(&api_key))
                    .multipart(form)
                    .send()
                    .await;

                match response {
                    // Success
                    Ok(response) if response.status().is_success() => {
                        match response.json::<UploadResponse>().await {
                            Ok(json_res) => {
                                let file_id = json_res.id;

                                // Add to all-file list.
                                file_ids.lock().await.push(file_id.clone());

                                // Add ONLY to this-run list.
                                new_file_ids.lock().await.push(file_id.clone());

                                // Save state
                                {
                                    let mut state = upload_state.lock().await;

                                    state.add_file(a_file.clone(), file_id, file_size);

                                    if let Some(path) = state_path.as_deref()
                                        && let Err(e) = state.save(path).await
                                    {
                                        progress_bar
                                            .overall_pb
                                            .println(format!("  ⚠ Failed to save state: {}", e));
                                    }
                                }

                                progress_bar.file_finished(&file_pb, filename, true);

                                // Delete local file
                                if delete && let Err(e) = tokio::fs::remove_file(&a_file).await {
                                    progress_bar.overall_pb.println(format!(
                                        "  ⚠ Failed to delete {}: {}",
                                        filename, e
                                    ));
                                }
                            }

                            Err(e) => {
                                progress_bar.file_finished(&file_pb, filename, false);

                                progress_bar.overall_pb.println(format!(
                                    "  ⚠ Invalid upload response for {}: {}",
                                    filename, e
                                ));
                            }
                        }
                    }

                    // Server error
                    Ok(response) => {
                        let status = response.status();
                        let body = response.text().await.unwrap_or_default();

                        progress_bar.file_finished(&file_pb, filename, false);

                        progress_bar.overall_pb.println(format!(
                            "  ⚠ Upload failed for {}: HTTP {}: {}",
                            filename, status, body
                        ));
                    }

                    // Request error
                    Err(e) => {
                        progress_bar.file_finished(&file_pb, filename, false);

                        progress_bar
                            .overall_pb
                            .println(format!("  ⚠ Upload request failed for {}: {}", filename, e));
                    }
                }
            });
        }

        while let Some(result) = set.join_next().await {
            if let Err(e) = result {
                eprintln!("  ⚠ Upload task failed: {}", e);
            }
        }
    }

    // Large files
    for (a_file, file_size) in large_files {
        // Check state
        let saved_file_id = {
            let state = upload_state.lock().await;

            state.get_file_id(&a_file, file_size).map(str::to_owned)
        };

        if let Some(file_id) = saved_file_id {
            let filename = a_file
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("file");

            println!("  ↻ [State File] Already uploaded, skipping: {}", filename);

            // Count skipped file in progress.
            let file_pb = progress_bar.file_started(filename, file_size);

            file_pb.set_position(file_size);

            if !progress_bar.is_single_file() {
                progress_bar.overall_pb.inc(file_size);
            }

            progress_bar.file_finished(&file_pb, filename, true);

            // Existing state file.
            file_ids.lock().await.push(file_id);

            // IMPORTANT:
            //
            // Not added to new_file_ids.
            continue;
        }

        let filename = a_file
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file");

        let file_pb = progress_bar.file_started(filename, file_size);

        // Opening file
        let file_handle = match tokio::fs::File::open(&a_file).await {
            Ok(file) => file,

            Err(e) => {
                progress_bar.file_finished(&file_pb, filename, false);

                progress_bar
                    .overall_pb
                    .println(format!("  ⚠ Failed to open {}: {}", filename, e));

                continue;
            }
        };

        // Streaming file
        let progress_reader = ProgressReader::new(
            file_handle,
            file_pb.clone(),
            progress_bar.overall_pb.clone(),
            !progress_bar.is_single_file(),
        );

        let stream = tokio_util::io::ReaderStream::new(progress_reader);

        let body = reqwest::Body::wrap_stream(stream);

        let part = reqwest::multipart::Part::stream_with_length(body, file_size)
            .file_name(filename.to_string());

        let form = reqwest::multipart::Form::new().part("file", part);

        // Upload
        let response = match http_c
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
                    .println(format!("  ⚠ Failed to upload {}: {}", filename, e));

                continue;
            }
        };

        let status = response.status();

        let body = match response.text().await {
            Ok(body) => body,

            Err(e) => {
                progress_bar.file_finished(&file_pb, filename, false);

                progress_bar.overall_pb.println(format!(
                    "  ⚠ Failed to read response for {}: {}",
                    filename, e
                ));

                continue;
            }
        };

        // Handling response
        if status.is_success() {
            match serde_json::from_str::<UploadResponse>(&body) {
                Ok(response_json) => {
                    let file_id = response_json.id;

                    // All files for potential new album.
                    file_ids.lock().await.push(file_id.clone());

                    // Only newly uploaded files.
                    new_file_ids.lock().await.push(file_id.clone());

                    // Save state
                    {
                        let mut state = upload_state.lock().await;

                        state.add_file(a_file.clone(), file_id, file_size);

                        if let Some(path) = state_path.as_deref()
                            && let Err(e) = state.save(path).await
                        {
                            progress_bar
                                .overall_pb
                                .println(format!("  ⚠ Failed to save state: {}", e));
                        }
                    }

                    progress_bar.file_finished(&file_pb, filename, true);

                    // Delete local file
                    if delete && let Err(e) = tokio::fs::remove_file(&a_file).await {
                        progress_bar
                            .overall_pb
                            .println(format!("  ⚠ Failed to delete {}: {}", filename, e));
                    }
                }

                Err(e) => {
                    progress_bar.file_finished(&file_pb, filename, false);

                    progress_bar.overall_pb.println(format!(
                        "  ⚠ Invalid upload response for {}: {}",
                        filename, e
                    ));
                }
            }
        } else {
            progress_bar.file_finished(&file_pb, filename, false);

            progress_bar.overall_pb.println(format!(
                "  ⚠ Upload failed for {}: HTTP {}: {}",
                filename, status, body
            ));
        }
    }

    progress_bar.all_finished();

    // Album handling
    let all_file_ids = file_ids.lock().await.clone();

    let uploaded_file_ids = new_file_ids.lock().await.clone();

    // ---------------------------------------------------------------
    // Case 1: Existing album
    // ---------------------------------------------------------------
    //
    // This can come from:
    //
    //   - state.album_id
    //   - --album-id
    //
    // ONLY newly uploaded files should be added.
    // ---------------------------------------------------------------

    if let Some(existing_album_id) = existing_album_id {
        if uploaded_file_ids.is_empty() {
            println!("  ✓ No new files to add to album.");
        } else {
            println!(
                "\nAdding {} new file(s) to album '{}'...",
                uploaded_file_ids.len(),
                existing_album_id
            );

            if let Err(e) = AlbumAction::add_files_to_album(
                &http_c,
                &api_key,
                &existing_album_id,
                &uploaded_file_ids,
            )
            .await
            {
                eprintln!("  ⚠ Failed to update album '{}': {}", existing_album_id, e);
            } else {
                println!("  ✓ Album updated successfully.");
            }
        }

        return Ok(());
    }

    // ---------------------------------------------------------------
    // Case 2: --album was supplied and no album exists yet.
    // ---------------------------------------------------------------
    //
    // We create a new album using ALL known file IDs.
    //
    // This is important if:
    //
    //   1. Files were previously uploaded with --state
    //   2. No album existed at that time
    //   3. User later runs --album "My Album"
    //
    // In that case, the album should contain the previous files too.
    // ---------------------------------------------------------------

    if let Some(album_name) = album {
        if all_file_ids.is_empty() {
            eprintln!("  ⚠ No files available for album creation.");
            return Ok(());
        }

        match create_album(&http_c, &api_key, album_name, &all_file_ids).await {
            Ok(new_album_id) => {
                println!("✓ Album created successfully!");
                println!("  https://pixeldrain.com/l/{}", new_album_id);

                // Save album ID into state
                if let Some(path) = state_path.as_deref() {
                    let mut state = upload_state.lock().await;

                    state.set_album_id(new_album_id);

                    if let Err(e) = state.save(path).await {
                        eprintln!("  ⚠ Failed to save album ID to state: {}", e);
                    } else {
                        println!("  ✓ Album ID saved to state.");
                    }
                }
            }

            Err(e) => {
                eprintln!("  ⚠ Failed to create album '{}': {}", album_name, e);
            }
        }
    }

    Ok(())
}

// Files collection
fn collect_files(paths: &[PathBuf], formats: Option<&[String]>) -> Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = Vec::new();

    for path in paths {
        if !path.exists() {
            bail!("Path Doesn't Exists !!! {}", path.display());
        }

        if path.is_file() {
            if matches_format(path, formats) {
                files.push(path.clone());
            }
        } else if path.is_dir() {
            collect_files_from_dir(path, formats, &mut files)?;
        } else {
            bail!("Path is neither a file nor a directory: {}", path.display());
        }
    }

    // Natural ordering.
    files.sort_by(|a, b| natord::compare(&a.to_string_lossy(), &b.to_string_lossy()));

    // Remove duplicates.
    //
    // Example:
    //
    // ./videos/1.mp4
    // ./videos/
    //
    // would otherwise find 1.mp4 twice.
    files.dedup();

    Ok(files)
}

fn collect_files_from_dir(
    directory: &Path,
    formats: Option<&[String]>,
    files: &mut Vec<PathBuf>,
) -> Result<()> {
    let dir_entries = std::fs::read_dir(directory)
        .with_context(|| format!("Failed to read directory: {}", directory.display()))?;

    for entry in dir_entries {
        let dir_entry = entry?;
        let path = dir_entry.path();

        if path.is_file() {
            if matches_format(&path, formats) {
                files.push(path);
            }
        } else if path.is_dir() {
            collect_files_from_dir(&path, formats, files)?;
        }
    }

    Ok(())
}

fn matches_format(path: &Path, formats: Option<&[String]>) -> bool {
    let Some(formats) = formats else {
        return true;
    };

    let Some(extension) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };

    let extension = extension.to_ascii_lowercase();

    formats.iter().any(|format| {
        format
            .trim_start_matches('.')
            .eq_ignore_ascii_case(&extension)
    })
}

// Album creation
async fn create_album(
    http_client: &Client,
    api_key: &str,
    album_name: &str,
    file_ids: &[String],
) -> Result<String> {
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

    println!(
        "\nCreating album '{}' with {} file(s)...",
        album_name,
        file_ids.len()
    );

    let response = http_client
        .post("https://pixeldrain.com/api/list")
        .basic_auth("", Some(api_key))
        .json(&payload)
        .send()
        .await?;

    let status = response.status();

    let body = response.text().await?;

    if !status.is_success() {
        bail!("Failed to create album: HTTP {}\n{}", status, body);
    }

    let response_json: serde_json::Value =
        serde_json::from_str(&body).context("Invalid album creation response")?;

    let album_id = response_json["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Album response did not contain an ID"))?;

    Ok(album_id.to_owned())
}

#[derive(Deserialize)]
struct UploadResponse {
    id: String,
}
