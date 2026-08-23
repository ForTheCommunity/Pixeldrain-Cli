use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::{
    auth,
    progress_bar::{ProgressReader, UploadProgress},
};

pub async fn upload(
    paths: &[PathBuf],
    album: Option<&str>,
    formats: Option<&[String]>,
    delete: bool,
) -> Result<()> {
    let api_key = auth::get_api_key()?;

    let files = collect_files(paths, formats)?;

    if files.is_empty() {
        println!("  No matching files found.");
        return Ok(());
    }

    let total_files = files.len();

    // threshold for concurrency.... i.e [ 20 MB ].
    const SMALL_FILE_THRESHOLD: u64 = 20 * 1024 * 1024;

    // small files
    let mut small_files: Vec<(PathBuf, u64)> = Vec::new();
    // files larger than threshold.
    let mut large_files: Vec<(PathBuf, u64)> = Vec::new();

    for file in files {
        if let Ok(meta_data) = tokio::fs::metadata(&file).await {
            if meta_data.len() < SMALL_FILE_THRESHOLD {
                small_files.push((file, meta_data.len()));
            } else {
                large_files.push((file, meta_data.len()));
            }
        }
    }

    println!("   ✦ Total Files : {}", total_files);

    let http_c = reqwest::Client::new();
    let total_bytes: u64 = small_files.iter().map(|(_, len)| len).sum::<u64>()
        + large_files.iter().map(|(_, len)| len).sum::<u64>();

    let progress_bar = UploadProgress::new(total_files, total_bytes);

    // uploaded file ids.
    let file_ids: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    // Concurrent uploads for small files....
    if !small_files.is_empty() {
        let mut set = tokio::task::JoinSet::new();

        for (a_file, file_size) in small_files {
            let client = http_c.clone();
            let api_key = api_key.clone();
            let progress_bar = progress_bar.clone();
            let file_ids = Arc::clone(&file_ids);

            set.spawn(async move {
                let filename = a_file
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("file");

                // progress bar
                let file_pb = progress_bar.file_started(filename, file_size);

                let bytes = match tokio::fs::read(&a_file).await {
                    Ok(b) => b,
                    Err(e) => {
                        progress_bar.file_finished(&file_pb, filename, false);
                        progress_bar
                            .overall_pb
                            .println(format!("  ⚠ Failed to read {}: {}", filename, e));
                        return;
                    }
                };

                // Manually update progress for small files since they read instantly
                file_pb.set_position(file_size);
                if !progress_bar.is_single_file() {
                    progress_bar.overall_pb.inc(file_size);
                }

                let part = reqwest::multipart::Part::bytes(bytes).file_name(filename.to_string());
                let form = reqwest::multipart::Form::new().part("file", part);

                let res = client
                    .post("https://pixeldrain.com/api/file")
                    .basic_auth("", Some(&api_key))
                    .multipart(form)
                    .send()
                    .await;

                match res {
                    Ok(response) if response.status().is_success() => {
                        if let Ok(json_res) = response.json::<UploadResponse>().await {
                            file_ids.lock().await.push(json_res.id);
                            progress_bar.file_finished(&file_pb, filename, true);

                            if delete {
                                let _ = tokio::fs::remove_file(&a_file).await;
                            }
                        } else {
                            progress_bar.file_finished(&file_pb, filename, false);
                        }
                    }
                    _ => {
                        progress_bar.file_finished(&file_pb, filename, false);
                    }
                }
            });
        }
        while let Some(_res) = set.join_next().await {}
    }

    // Uploading large files sequentially.
    for (a_file, file_size) in large_files {
        let filename = a_file
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file");

        let file_pb = progress_bar.file_started(filename, file_size);

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

        if status.is_success() {
            if let Ok(response_json) = serde_json::from_str::<UploadResponse>(&body) {
                file_ids.lock().await.push(response_json.id);
                progress_bar.file_finished(&file_pb, filename, true);

                if delete {
                    let _ = tokio::fs::remove_file(&a_file).await;
                }
            } else {
                progress_bar.file_finished(&file_pb, filename, false);
            }
        } else {
            progress_bar.file_finished(&file_pb, filename, false);
        }
    }

    progress_bar.all_finished();

    // Create album with successfully uploaded files.
    let final_file_ids = file_ids.lock().await;
    if let Some(album_name) = album {
        if !final_file_ids.is_empty() {
            if let Err(e) = create_album(&http_c, &api_key, album_name, &final_file_ids).await {
                eprintln!("  ⚠ Failed to create album '{}': {}", album_name, e);
            }
        }
    }

    Ok(())
}

fn collect_files(paths: &[PathBuf], formats: Option<&[String]>) -> Result<Vec<PathBuf>> {
    // ALL FILES
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

    // Natural Ordering of Files
    files.sort_by(|a, b| natord::compare(&a.to_string_lossy(), &b.to_string_lossy()));
    // removing duplicate entries
    // eg. ./videos/1.mp4 & ./videos/
    // there will be two 1.mp4 so removing duplicate entries...
    files.dedup();

    Ok(files)
}

fn collect_files_from_dir(
    directory: &Path,
    formats: Option<&[String]>,
    files: &mut Vec<PathBuf>,
) -> Result<()> {
    let dir_entries = std::fs::read_dir(directory)
        .with_context(|| format!("Failed to read directory: {} ", directory.display()))?;

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

async fn create_album(
    http_client: &Client,
    api_key: &str,
    album_name: &str,
    file_ids: &[String],
) -> Result<()> {
    println!("");

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
        anyhow::bail!("Failed to create album: HTTP {}\n{}", status, body);
    }

    let response_json: serde_json::Value = serde_json::from_str(&body)?;

    if let Some(album_id) = response_json["id"].as_str() {
        println!("✓ Album created successfully!");
        println!("  https://pixeldrain.com/l/{}", album_id);
    } else {
        anyhow::bail!("Album response did not contain an ID");
    }

    Ok(())
}

#[derive(Deserialize)]
struct UploadResponse {
    id: String,
}
