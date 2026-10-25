use std::path::PathBuf;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use anyhow::{Result, anyhow};
use indicatif::{ProgressBar, ProgressStyle};
use tokio::fs::{self, File};
use tokio::io::{AsyncRead, ReadBuf};
use transformer::crypt::{encrypt_file_ks, encyrpt_file_wp};

use crate::core::hash::compute_b3hash;
use crate::core::password::prompt_password;
use crate::{
    api::endpoints::album::AlbumApi,
    app::uploader::UploadPipeline,
    core::{
        collector::collect_files, state::upload::enc_state::EncUploadState,
        storage::project_data_dir,
    },
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
    if let Some(ref al_name) = upload_options.album_name {
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
        Some(state_path) if state_path.exists() => match EncUploadState::load(state_path).await {
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
                EncUploadState::default()
            }
        },
        _ => EncUploadState::default(),
    };

    // Prepare temp directory once outside the loop
    let temp_dir = project_data_dir()?.join("temp_files");
    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir).await?;
    }
    fs::create_dir_all(&temp_dir).await?;

    // Master password cache for keystore (asked only once)
    let mut ks_master_password: Option<String> = None;
    // Password cache for password-based encryption (asked only once)
    let mut enc_password: Option<String> = match &upload_options.enc_password {
        Some(Some(pwd)) => Some(pwd.clone()),
        _ => None,
    };

    // upload loop
    for file in all_files {
        let metadata = match fs::metadata(&file).await {
            Ok(metadata) => metadata,
            Err(e) => {
                eprintln!("  ⚠ Failed to read metadata for {}: {}", file.display(), e);
                continue;
            }
        };

        // Prompt for master password only if it hasn't been provided yet
        if ks_master_password.is_none() && upload_options.enc_keystore.is_some() {
            let password = prompt_password(" Enter Your Master Password ->  ")?;
            ks_master_password = Some(password);
        }

        // Prompt for encryption password only if it hasn't been provided yet
        if enc_password.is_none() && upload_options.enc_password.is_some() {
            let password = prompt_password(" Enter Encryption Password ->  ")?;
            enc_password = Some(password);
        }

        let file_size = metadata.len(); // Original file size for state tracking
        let orignal_filename = file.file_name().unwrap().to_str().unwrap().to_owned();

        // chunk size.
        // using same chunk/buffer size for hashing and encryption.
        let chunk_size = (upload_options.chunk_size as usize) * 1024 * 1024;

        // State check
        // skip files that were already uploaded with the same size & hash.

        if upload_state
            .file_lookup(&orignal_filename, file_size, &file, chunk_size)
            .await?
        {
            println!(
                "Skipped file {:?}, already uploaded....",
                file.file_name().unwrap()
            );
            continue;
        }

        // computing blake3 Hash of orignal file.
        let original_file_b3hash = compute_b3hash(chunk_size, file_size, &file).await?;

        // Setup Progress Bar for Encryption
        let enc_pb = ProgressBar::new(file_size);
        enc_pb.set_style(
            ProgressStyle::default_bar()
                .template("  {spinner:.cyan} Encrypting [{bar:40.cyan/dim}] {bytes}/{total_bytes} (ETA: {eta})")
                .unwrap()
                .progress_chars("━╸─"),
        );
        enc_pb.enable_steady_tick(Duration::from_millis(100));

        // Encryption
        let enc_res = if let Some(keystore_path) = &upload_options.enc_keystore {
            let out_path = temp_dir.clone();
            let pwd = ks_master_password
                .as_ref()
                .ok_or_else(|| anyhow!("Master password not provided"))?
                .clone();
            let enc_pb_clone = enc_pb.clone();

            encrypt_file_ks(
                keystore_path,
                &file,
                &out_path,
                chunk_size,
                pwd,
                false,
                move |bytes_read| {
                    enc_pb_clone.inc(bytes_read);
                },
            )
        } else if upload_options.enc_password.is_some() {
            let pwd = enc_password
                .as_ref()
                .ok_or_else(|| anyhow!("Encryption password not provided"))?
                .clone();
            let enc_pb_clone = enc_pb.clone();

            encyrpt_file_wp(
                &file,
                &temp_dir,
                pwd,
                chunk_size,
                false,
                move |bytes_read| {
                    enc_pb_clone.inc(bytes_read);
                },
            )
        } else {
            return Err(anyhow!(
                "Error: Neither an encryption password nor a keystore was provided."
            ));
        };

        let encrypted_file: PathBuf = match enc_res {
            Ok(path) => {
                enc_pb.finish_and_clear();
                path
            }
            Err(e) => {
                enc_pb.abandon_with_message("Encryption failed");
                return Err(e);
            }
        };

        let file_reader = File::open(&encrypted_file).await?;

        // Get the length of the *encrypted* file for streaming & progress bar total
        let encrypted_metadata = fs::metadata(&encrypted_file).await?;
        let encrypted_file_length = encrypted_metadata.len();

        let encrypted_file_name = encrypted_file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("encrypted_file");

        // Setup Progress Bar
        let pb = ProgressBar::new(encrypted_file_length);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} {msg} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                .unwrap()
                .progress_chars("=>-"),
        );

        // Wrap reader with progress bar tracker
        let progress_reader = ProgressReader {
            inner: file_reader,
            pb: pb.clone(),
        };

        // uploading....
        println!(
            "  Uploading: '{}' as '{}'",
            orignal_filename, encrypted_file_name
        );
        let api_client = upload_pipeline.client.clone();

        let upload_response = match api_client
            .upload_file_stream(encrypted_file_name, progress_reader, encrypted_file_length)
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

        // blake3 hash of orignal file.
        let enc_file_name = encrypted_file
            .file_name()
            .ok_or(anyhow!(
                "Error occurred while getting filename of encrypted file."
            ))?
            .to_str()
            .ok_or(anyhow!("Error converting OsString to String"))?
            .to_owned();

        // Record uploaded file into state using the original `file_size`
        upload_state.add_file(
            orignal_filename,
            file_id.clone(),
            file_size,
            original_file_b3hash,
            enc_file_name,
        );
        let state_path = upload_options.state.clone();
        save_state(&upload_state, &state_path).await;

        // album works
        if upload_options.album_id.is_some()
            || upload_options.album_name.is_some()
            || upload_state.album_id.is_some()
        {
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
        }

        // deleting orignal file if delete flag is passed.
        if upload_options.delete_after {
            match fs::remove_file(&file).await {
                Ok(_) => {
                    println!("  🗑 Deleted orignal file: {}", file.display());
                }
                Err(e) => {
                    println!(
                        "  ⚠ Failed to delete orignal file {}: {}",
                        file.display(),
                        e
                    );
                }
            }
        }
    }

    // Print the current album ID and URL once it's created or updated
    if let Some(ref album_id) = upload_state.album_id {
        println!(
            " Album ID: {} | URL -> https://pixeldrain.com/l/{}",
            album_id, album_id
        );
    }

    Ok(())
}

async fn save_state(upload_state: &EncUploadState, state_path: &Option<PathBuf>) {
    if let Some(path) = state_path {
        if let Err(e) = upload_state.save(path).await {
            eprintln!("  ⚠ Failed to save state: {:#}", e);
        }
    }
}
