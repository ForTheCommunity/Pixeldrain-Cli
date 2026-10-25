use anyhow::{Result, anyhow};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::time::Duration;
use tokio::fs::File;
use transformer::crypt::{check_master_password, decyrpt_file_ks, decyrpt_file_wp};

use crate::{
    api::client::ApiClient,
    core::{
        password::prompt_password,
        progress::{ProgressReader, TransferProgress, TransferType},
    },
};

pub struct Downloader {
    pub urls: Vec<String>,
    pub output_path: PathBuf,
    pub keystore_path: Option<PathBuf>,
    pub password: Option<Option<String>>,
}

impl Downloader {
    pub async fn start(self, api_client: ApiClient) -> Result<()> {
        if !self.output_path.exists() {
            return Err(anyhow!("Output Path doesn't exist."));
        }

        let mut password: Option<String> = None;

        if self.keystore_path.is_some() || self.password.is_some() {
            if let Some(ks_path) = self.keystore_path.as_ref() {
                let pwd_input = prompt_password(" Enter Master Password : ")?;
                match check_master_password(ks_path, &pwd_input) {
                    Ok(res) => {
                        if res {
                            password = Some(pwd_input);
                        } else {
                            eprintln!(" Enter correct Master Password for keystore !!!");
                            return Err(anyhow!("Incorrect master password"));
                        }
                    }
                    Err(e) => return Err(anyhow!("Error -> {}", e)),
                }
            } else if let Some(inner_opt) = self.password.as_ref() {
                if let Some(pwd) = inner_opt {
                    password = Some(pwd.clone());
                } else {
                    let pwd_input = prompt_password(" Enter Password : ")?;
                    password = Some(pwd_input);
                }
            } else {
                return Err(anyhow!("None of the possible cases matched....."));
            }
        }

        for url in self.urls {
            // Extract file ID from URL (e.g., https://pixeldrain.com/u/abc123xyz -> abc123xyz)
            let file_id = match url.split("/u/").last() {
                Some(id) if !id.is_empty() => id,
                _ => return Err(anyhow!("Invalid pixeldrain URL: {}", url)),
            };

            // Fetch file info to get the original filename
            let file_info = api_client.get_file_info(file_id).await?;
            let filename = file_info.name;

            // Determine final and temporary file paths
            let (dest_file_path, temp_file_path) = if self.output_path.is_dir() {
                let dest = self.output_path.join(&filename);
                let temp = self.output_path.join(format!("{}_tmp", &filename));
                (dest, temp)
            } else {
                let dest = self.output_path.clone();
                let parent = dest.parent().unwrap_or_else(|| std::path::Path::new("."));
                let file_name = dest
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("download");
                let temp = parent.join(format!("{}_tmp", file_name));
                (dest, temp)
            };

            // Call the download function stream
            let (total_length, reader) = api_client.download_file_stream(file_id).await?;

            // Setup TransferProgress manager for the download using the real filename
            let transfer_progress = TransferProgress::new(TransferType::Download, 1, total_length);
            let file_pb = transfer_progress.file_started(&filename, total_length);

            // Wrap reader using the ProgressReader::new constructor
            let mut progress_reader = ProgressReader::new(
                reader,
                file_pb.clone(),
                transfer_progress.overall_pb.clone(),
                !transfer_progress.is_single_file(),
            );

            // Create the temporary destination file
            let mut dest_file = File::create(&temp_file_path).await?;

            // Stream data from the API response into the temporary file
            let copy_result = tokio::io::copy(&mut progress_reader, &mut dest_file).await;

            // Explicitly drop the file handle so it's closed before rename/delete operations
            drop(dest_file);

            match copy_result {
                Ok(_) => {
                    // Rename temp file to final filename once 100% complete
                    tokio::fs::rename(&temp_file_path, &dest_file_path).await?;
                    transfer_progress.file_finished(&file_pb, &filename, true);

                    // ending progress bar so that 'decyrpt_file_ks' can prompt for master password
                    transfer_progress.all_finished();

                    // decrypting file (if password or keystore was provided)
                    if self.password.is_some() || self.keystore_path.is_some() {
                        let file_path = &dest_file_path;
                        let file_size = match tokio::fs::metadata(file_path).await {
                            Ok(meta) => meta.len(),
                            Err(_) => total_length,
                        };

                        let dec_pb = ProgressBar::new(file_size);
                        dec_pb.set_style(
                            ProgressStyle::default_bar()
                                .template("  {spinner:.cyan} Decrypting [{bar:40.cyan/dim}] {bytes}/{total_bytes} (ETA: {eta})")
                                .unwrap()
                                .progress_chars("━╸─"),
                        );
                        dec_pb.enable_steady_tick(Duration::from_millis(100));

                        let dec_pb_clone = dec_pb.clone();
                        let pwd = password
                            .as_ref()
                            .ok_or_else(|| anyhow!("Password not provided for decryption"))?
                            .clone();

                        let dec_res = if let Some(keystore_path) = &self.keystore_path {
                            decyrpt_file_ks(
                                keystore_path,
                                file_path,
                                &self.output_path,
                                pwd,
                                true,
                                move |bytes_read| {
                                    dec_pb_clone.inc(bytes_read);
                                },
                            )
                        } else {
                            decyrpt_file_wp(
                                file_path,
                                &self.output_path,
                                pwd,
                                true,
                                move |bytes_read| {
                                    dec_pb_clone.inc(bytes_read);
                                },
                            )
                        };

                        match dec_res {
                            Ok(restored_path) => {
                                dec_pb.finish_and_clear();
                                let restored_name = restored_path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or(&filename);
                                println!(
                                    "  ✓ Successfully decrypted! Restored original file: '{}'",
                                    restored_name
                                );
                            }
                            Err(e) => {
                                dec_pb.abandon_with_message("Decryption failed");
                                return Err(e);
                            }
                        }
                    }
                }
                Err(e) => {
                    // Clean up partial temporary file on failure
                    let _ = tokio::fs::remove_file(&temp_file_path).await;
                    transfer_progress.file_finished(&file_pb, &filename, false);
                    return Err(e.into());
                }
            }
        }

        Ok(())
    }
}
