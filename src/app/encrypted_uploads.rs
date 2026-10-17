use std::path::PathBuf;
use std::pin::Pin;
use std::task::{Context, Poll};

use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use tokio::fs::{self, File};
use tokio::io::{AsyncRead, ReadBuf};
use transformer::crypt::{encrypt_file_ks, encyrpt_file_wp};

use crate::core::password::prompt_password;
use crate::{
    api::endpoints::album::AlbumApi,
    app::uploader::UploadPipeline,
    core::{collector::collect_files, state::upload::UploadState, storage::project_data_dir},
};

/// A wrapper around any tokio AsyncRead that updates an indicatif ProgressBar as it's read.
struct ProgressReader<R> {
    inner: R,
    pb: ProgressBar,
}

impl<R: AsyncRead + Unpin> AsyncRead for ProgressReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let prev_filled = buf.filled().len();
        match Pin::new(&mut self.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                let bytes_read = (buf.filled().len() - prev_filled) as u64;
                if bytes_read > 0 {
                    self.pb.inc(bytes_read);
                }
                Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}

pub async fn encrypted_upload(upload_pipeline: UploadPipeline) -> Result<()> {
    let upload_options = upload_pipeline.options;

    let mut album_name_cli = String::new();
    if let Some(al_name) = upload_options.album_name {
        album_name_cli = al_name.join(" ");
        println!("Album Name : {}", album_name_cli);
    };

    // scanning files & filtering files,
    let all_files = collect_files(&upload_options.paths, upload_options.formats.as_deref())?;

    if all_files.is_empty() {
        if let Some(filters) = &upload_options.formats {
            let formats = filters
                .iter()
                .map(|extension| {
                    if extension.starts_with(".") {
                        extension.clone()
                    } else {
                        format!(".{}", extension)
                    }
                })
                .collect::<Vec<String>>()
                .join(", ");
            println!("  No files found with format : {formats}");
            return Ok(());
        } else {
            println!("  No files found in given path...");
            return Ok(());
        }
    }

    // Load or create upload state
    let mut upload_state = match &upload_options.state {
        Some(state_path) if state_path.exists() => match UploadState::load(state_path).await {
            Ok(state) => {
                println!("  ✦ Resuming from state file: {}", state_path.display());
                state
            }
            Err(e) => {
                eprintln!(
                    "  ⚠ Failed to load state file ({}): {:#}. Starting fresh.",
                    state_path.display(),
                    e
                );
                UploadState::default()
            }
        },
        _ => UploadState::default(),
    };

    // Prepare temp directory once outside the loop
    let temp_dir = project_data_dir()?.join("temp_files");
    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir).await?;
    }
    fs::create_dir_all(&temp_dir).await?;

    // Master password cache for keystore (asked only once)
    let mut ks_master_password: Option<String> = None;

    // upload loop
    for file in all_files {
        let metadata = match fs::metadata(&file).await {
            Ok(metadata) => metadata,
            Err(e) => {
                eprintln!("  ⚠ Failed to read metadata for {}: {}", file.display(), e);
                continue;
            }
        };

        let file_size = metadata.len(); // Original file size for state tracking

        // skip files that were already uploaded with the same size.
        if let Some(_existing_id) = upload_state.get_file_id(&file, file_size) {
            println!(
                "Skipped file {:?}, already uploaded....",
                file.file_name().unwrap()
            );
            continue;
        }

        let chunk_size = upload_options.chunk_size as usize;

        let encrypted_file: PathBuf = if let Some(keystore_path) = &upload_options.enc_keystore {
            let out_path = temp_dir.join(file.file_name().unwrap());

            // Prompt for master password only if it hasn't been provided yet
            if ks_master_password.is_none() {
                let password = prompt_password(" Enter Your Master Password ->  ")?;
                ks_master_password = Some(password);
            }

            encrypt_file_ks(
                keystore_path,
                &file,
                &out_path,
                chunk_size,
                ks_master_password.clone(),
            )?;
            out_path
        } else if let Some(password) = &upload_options.enc_password {
            encyrpt_file_wp(&file, &temp_dir, password, chunk_size)?
        } else {
            return Err(anyhow::anyhow!(
                "Error: Neither an encryption password nor a keystore was provided."
            ));
        };

        let file_reader = File::open(&encrypted_file).await?;

        // Get the length of the *encrypted* file for streaming & progress bar total
        let encrypted_metadata = fs::metadata(&encrypted_file).await?;
        let encrypted_length = encrypted_metadata.len();

        let filename = encrypted_file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("encrypted_file");

        // Setup Progress Bar
        let pb = ProgressBar::new(encrypted_length);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                .unwrap()
                .progress_chars("#>-"),
        );

        // Wrap reader with progress bar tracker
        let progress_reader = ProgressReader {
            inner: file_reader,
            pb: pb.clone(),
        };

        // uploading....
        let api_client = upload_pipeline.client.clone();
        let upload_response = match api_client
            .upload_file_stream(filename, progress_reader, encrypted_length)
            .await
        {
            Ok(resp) => {
                pb.finish_with_message("Uploaded successfully!");
                let _ = fs::remove_file(&encrypted_file).await;
                resp
            }
            Err(e) => {
                pb.abandon_with_message("Upload failed");
                return Err(e.into());
            }
        };

        println!(
            " File uploaded, URL -> https://pixeldrain.com/u/{}",
            upload_response.id
        );

        let file_id = upload_response.id;

        // Record uploaded file into state using the original `file_size`
        upload_state.add_file(file, file_id.clone(), file_size);
        let state_path = upload_options.state.clone();
        save_state(&upload_state, &state_path).await;

        // Safe API key handling
        let api_key = api_client
            .api_key
            .as_deref()
            .expect("API key is required for album creation and management");

        let album_api = AlbumApi {
            api_key,
            id: upload_state.album_id.as_deref(),
        };

        match upload_state.album_id.clone() {
            Some(album_id_str) => {
                // updating album..
                let album_api = AlbumApi {
                    api_key,
                    id: Some(&album_id_str),
                };

                AlbumApi::add_file(album_api, &[file_id]).await?;
            }
            None => {
                // creating new album & updating album id in state file.
                match AlbumApi::create_album(&album_api, &album_name_cli, &[file_id]).await {
                    Ok(new_album_id) => {
                        // saving album id to state
                        upload_state.set_album_id(new_album_id);
                    }
                    Err(e) => {
                        println!(" Error -> {}", e.to_string())
                    }
                }
            }
        }

        // Print the current album ID and URL once it's created or updated
        if let Some(ref album_id) = upload_state.album_id {
            println!(
                " Album ID: {} | URL -> https://pixeldrain.com/a/{}",
                album_id, album_id
            );
        }
    }

    Ok(())
}

async fn save_state(upload_state: &UploadState, state_path: &Option<PathBuf>) {
    if let Some(path) = state_path {
        if let Err(e) = upload_state.save(path).await {
            eprintln!("  ⚠ Failed to save state: {:#}", e);
        }
    }
}
