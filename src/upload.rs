use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde::Deserialize;

use crate::{
    auth,
    progress_bar::{ProgressReader, UploadProgress},
};

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

    let mut total_bytes: u64 = 0;
    for file in &files {
        if let Ok(meta) = tokio::fs::metadata(file).await {
            total_bytes += meta.len();
        }
    }

    println!("   ✦ Total Files : {}", total_files);

    // uploading
    let client = reqwest::Client::new();
    // progress bar
    let progress_bar = UploadProgress::new(total_files, total_bytes);

    let mut file_ids: Vec<String> = Vec::new();
    for a_file in files {
        let filename = a_file
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file");

        // file size for progress bar.
        let file_size = match tokio::fs::metadata(&a_file).await {
            Ok(metadata) => metadata.len(),
            Err(e) => {
                progress_bar.overall_pb.println(format!(
                    "  ⚠ Skipped {}: unable to read file metadata: {}",
                    filename, e
                ));
                continue;
            }
        };

        // creating a progress bar per file.
        let file_pb = progress_bar.file_started(filename, file_size);

        // opening file and wrapping it in a ProgressReader so the bar
        // increases automatically as reqwest streams the body.
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
                    "  ⚠ Failed to read upload response for {}: {}",
                    filename, e
                ));

                continue;
            }
        };

        if status.is_success() {
            let response_json: UploadResponse = match serde_json::from_str(&body) {
                Ok(json) => json,
                Err(e) => {
                    progress_bar.file_finished(&file_pb, filename, false);

                    if delete {
                        progress_bar.overall_pb.println(format!(
                            "  ⚠ Upload succeeded for {}, but the server returned an invalid response. Local file was NOT deleted: {}",
                            filename, e
                        ));
                    } else {
                        progress_bar.overall_pb.println(format!(
                            "  ⚠ Upload succeeded for {}, but the server returned an invalid response: {}",
                            filename, e
                        ));
                    }

                    continue;
                }
            };

            file_ids.push(response_json.id.clone());
            progress_bar.file_finished(&file_pb, filename, true);

            // Delete only after a successful upload with a valid file ID.
            if delete {
                match tokio::fs::remove_file(&a_file).await {
                    Ok(_) => {
                        progress_bar
                            .overall_pb
                            .println(format!("  ✓ Deleted: {}", a_file.display()));
                    }
                    Err(e) => {
                        progress_bar.overall_pb.println(format!(
                            "  ⚠ Failed to delete {}: {}",
                            a_file.display(),
                            e
                        ));
                    }
                }
            }
        } else {
            progress_bar.file_finished(&file_pb, filename, false);
            progress_bar.overall_pb.println(format!(
                "  ⚠ Upload failed for '{}': HTTP {} – {}",
                filename, status, body
            ));
        }
    }

    progress_bar.all_finished();

    // Create album with successfully uploaded files.
    if let Some(album_name) = album {
        if !file_ids.is_empty() {
            if let Err(e) = create_album(&client, &api_key, album_name, &file_ids).await {
                eprintln!("  ⚠ Failed to create album '{}': {}", album_name, e);
            }
        }
    }

    Ok(())
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
