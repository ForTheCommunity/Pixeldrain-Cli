use anyhow::{Context, Result, anyhow};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::time::Duration;
use tokio::{fs::File, io::AsyncWriteExt};
use transformer::crypt::{check_master_password, decrypt_file_ks, decrypt_file_wp};

use crate::{
    api::{client::ApiClient, endpoints::album::AlbumApi},
    core::{
        format_bytes,
        hash::{Blake3Writer, compute_b3hash, compute_b3hash_with_pb},
        password::resolve_secret,
        progress::{ProgressReader, TransferProgress, TransferType},
        state::download::DownloadState,
    },
};

struct DownloadItem {
    id: String,
    name: String,
    size: u64,
}

#[derive(Debug, PartialEq, Eq)]
enum UrlTarget {
    File(String),
    Album(String),
    Id(String),
}

fn parse_url_target(url: &str) -> Result<UrlTarget> {
    let clean = url.trim();

    if clean.contains("/l/") || clean.contains("/list/") {
        let raw_id = if clean.contains("/l/") {
            clean.split("/l/").last()
        } else {
            clean.split("/list/").last()
        };

        let id = raw_id
            .and_then(|s| s.split(['?', '#', '/']).find(|p| !p.is_empty()))
            .ok_or_else(|| anyhow!("Invalid pixeldrain album URL: {}", url))?;

        Ok(UrlTarget::Album(id.to_string()))
    } else if clean.contains("/u/") {
        let raw_id = clean.split("/u/").last();

        let id = raw_id
            .and_then(|s| s.split(['?', '#', '/']).find(|p| !p.is_empty()))
            .ok_or_else(|| anyhow!("Invalid pixeldrain URL: {}", url))?;

        Ok(UrlTarget::File(id.to_string()))
    } else if !clean.contains('/') && !clean.is_empty() {
        // Fallback for raw ID (could be a file or an album)
        let id = clean.split(['?', '#']).next().unwrap_or(clean);
        if id.is_empty() {
            return Err(anyhow!("Invalid pixeldrain URL: {}", url));
        }
        Ok(UrlTarget::Id(id.to_string()))
    } else {
        Err(anyhow!("Invalid pixeldrain URL: {}", url))
    }
}

async fn fetch_file_item(api_client: &ApiClient, file_id: &str) -> Result<DownloadItem> {
    let file_info = api_client
        .get_file_info(file_id)
        .await
        .with_context(|| format!("Failed to fetch file info for '{}'", file_id))?;
    Ok(DownloadItem {
        id: file_id.to_string(),
        name: file_info.name,
        size: file_info.size,
    })
}

async fn fetch_album_items(api_key: &str, album_id: &str) -> Result<Vec<DownloadItem>> {
    let album_api = AlbumApi {
        api_key,
        id: Some(album_id),
    };
    let album_details = album_api
        .get_files()
        .await
        .with_context(|| format!("Failed to fetch files for album '{}'", album_id))?;

    println!(
        "  ✦ Album: '{}' ({} files)",
        album_details.title,
        album_details.files.len()
    );

    Ok(album_details
        .files
        .into_iter()
        .map(|file| DownloadItem {
            id: file.id,
            name: file.name,
            size: file.size,
        })
        .collect())
}

pub struct Downloader {
    pub urls: Vec<String>,
    pub output_path: PathBuf,
    pub keystore_path: Option<PathBuf>,
    pub keystore_pass: Option<String>,
    pub password: Option<Option<String>>,
    pub state: Option<PathBuf>,
    pub chunk_size: u8,
}

impl Downloader {
    pub async fn start(self, api_client: ApiClient) -> Result<()> {
        if !self.output_path.exists() {
            return Err(anyhow!("Output Path doesn't exist."));
        }

        let mut password: Option<String> = None;

        if self.keystore_path.is_some() || self.password.is_some() {
            if let Some(ks_path) = self.keystore_path.as_ref() {
                let pwd_input = resolve_secret(
                    self.keystore_pass.as_deref(),
                    "PIXELDRAIN_KEYSTORE_PASS",
                    " Enter Master Password : ",
                )?;
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
                let pwd_input = resolve_secret(
                    inner_opt.as_deref(),
                    "PIXELDRAIN_DECRYPT_PASS",
                    " Enter Password : ",
                )?;
                password = Some(pwd_input);
            } else {
                return Err(anyhow!("None of the possible cases matched....."));
            }
        }

        let api_key = api_client
            .api_key
            .as_deref()
            .map(|s| s.as_str())
            .unwrap_or("");

        let mut items = Vec::new();

        for url in self.urls {
            match parse_url_target(&url)? {
                UrlTarget::File(file_id) => {
                    items.push(fetch_file_item(&api_client, &file_id).await?);
                }
                UrlTarget::Album(album_id) => {
                    items.extend(fetch_album_items(api_key, &album_id).await?);
                }
                UrlTarget::Id(raw_id) => {
                    // Try resolving as a file first; fallback to album if not found
                    match fetch_file_item(&api_client, &raw_id).await {
                        Ok(item) => items.push(item),
                        Err(_) => match fetch_album_items(api_key, &raw_id).await {
                            Ok(album_items) => items.extend(album_items),
                            Err(_) => {
                                return Err(anyhow!(
                                    "Could not find any file or album with ID '{}'",
                                    raw_id
                                ));
                            }
                        },
                    }
                }
            }
        }

        if items.is_empty() {
            println!("  No files found to download.");
            return Ok(());
        }

        if items.len() > 1 && !self.output_path.is_dir() {
            return Err(anyhow!(
                "Output path must be a directory when downloading multiple files."
            ));
        }

        // Load or create download state
        let mut download_state = match &self.state {
            Some(state_path) if state_path.exists() => {
                match DownloadState::load(state_path).await {
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
                        DownloadState::default()
                    }
                }
            }
            _ => DownloadState::default(),
        };

        let chunk_size = (self.chunk_size.max(1) as usize) * 1024 * 1024;
        let is_decrypting = self.password.is_some() || self.keystore_path.is_some();

        let mut items_to_download = Vec::with_capacity(items.len());
        let mut skipped_count = 0usize;

        for item in items {
            if self.state.is_some()
                && download_state
                    .file_lookup(&item.id, &self.output_path, is_decrypting, chunk_size)
                    .await?
            {
                skipped_count += 1;
                continue;
            }
            items_to_download.push(item);
        }

        if skipped_count > 0 {
            println!(
                "  ✦ Skipped {} already-downloaded file(s) (from state)",
                skipped_count
            );
        }

        if items_to_download.is_empty() {
            return Ok(());
        }

        let total_files = items_to_download.len();
        let total_transfer_size: u64 = items_to_download.iter().map(|item| item.size).sum();

        println!(
            "   ✦ Files to download : {} \n   ✦ Download size     : {}",
            total_files,
            format_bytes(total_transfer_size)
        );

        let is_multi = total_files > 1;

        let transfer_progress = if is_multi {
            Some(TransferProgress::new(
                TransferType::Download,
                total_files,
                total_transfer_size,
            ))
        } else {
            None
        };

        for item in items_to_download {
            let filename = item.name;
            let file_id = item.id;

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
            let (total_length, reader) = api_client.download_file_stream(&file_id).await?;
            let file_length = if total_length > 0 {
                total_length
            } else {
                item.size
            };

            let single_progress;
            let active_progress = match &transfer_progress {
                Some(p) => p,
                None => {
                    single_progress = TransferProgress::new(TransferType::Download, 1, file_length);
                    &single_progress
                }
            };

            let file_pb = active_progress.file_started(&filename, file_length);

            // Wrap reader using the ProgressReader::new constructor
            let mut progress_reader = ProgressReader::new(
                reader,
                file_pb.clone(),
                active_progress.overall_pb.clone(),
                !active_progress.is_single_file(),
            );

            // Create the temporary destination file wrapped with streaming BLAKE3 (for unencrypted state)
            let dest_file = File::create(&temp_file_path).await?;
            let compute_b3_on_fly = self.state.is_some() && !is_decrypting;
            let mut blake3_writer = Blake3Writer::new(dest_file, compute_b3_on_fly);

            // Stream data from the API response into the temporary file
            let copy_result = tokio::io::copy(&mut progress_reader, &mut blake3_writer).await;

            // Explicitly drop the reader so the HTTP stream is closed
            drop(progress_reader);

            match copy_result {
                Ok(_) => {
                    blake3_writer.flush().await?;
                    let (dest_file, streamed_b3hash) = blake3_writer.finalize();
                    drop(dest_file);

                    // Rename temp file to final filename once 100% complete
                    tokio::fs::rename(&temp_file_path, &dest_file_path).await?;
                    active_progress.file_finished(&file_pb, &filename, true);

                    // decrypting file (if password or keystore was provided)
                    let (final_local_path, final_file_size) = if is_decrypting {
                        let file_path = &dest_file_path;
                        let file_size = match tokio::fs::metadata(file_path).await {
                            Ok(meta) => meta.len(),
                            Err(_) => file_length,
                        };

                        let dec_pb = if let Some(p) = &transfer_progress {
                            p.multi_pb
                                .insert_before(&p.overall_pb, ProgressBar::new(file_size))
                        } else {
                            ProgressBar::new(file_size)
                        };
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
                            // decyrpt_file_ks
                            decrypt_file_ks(
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
                            decrypt_file_wp(
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
                                if let Some(p) = &transfer_progress {
                                    p.multi_pb.remove(&dec_pb);
                                }
                                let restored_name = restored_path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or(&filename);
                                let dec_msg = format!(
                                    "  ✓ Successfully decrypted! Restored original file: '{}'",
                                    restored_name
                                );
                                if let Some(p) = &transfer_progress {
                                    p.overall_pb.println(dec_msg);
                                } else {
                                    println!("{}", dec_msg);
                                }
                                let restored_size = tokio::fs::metadata(&restored_path)
                                    .await
                                    .map(|m| m.len())
                                    .unwrap_or(0);
                                (restored_path, restored_size)
                            }
                            Err(e) => {
                                dec_pb.abandon_with_message("Decryption failed");
                                if let Some(p) = &transfer_progress {
                                    p.multi_pb.remove(&dec_pb);
                                }
                                return Err(e);
                            }
                        }
                    } else {
                        let downloaded_size = tokio::fs::metadata(&dest_file_path)
                            .await
                            .map(|m| m.len())
                            .unwrap_or(file_length);
                        (dest_file_path.clone(), downloaded_size)
                    };

                    if !is_multi {
                        active_progress.all_finished();
                    }

                    if let Some(state_path) = &self.state {
                        let file_b3hash = match streamed_b3hash {
                            Some(b3) => b3,
                            None => {
                                if let Some(p) = &transfer_progress {
                                    compute_b3hash_with_pb(
                                        chunk_size,
                                        final_file_size,
                                        &final_local_path,
                                        Some((&p.multi_pb, &p.overall_pb)),
                                    )
                                    .await?
                                } else {
                                    compute_b3hash(chunk_size, final_file_size, &final_local_path)
                                        .await?
                                }
                            }
                        };

                        let local_file_name = final_local_path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| filename.clone());

                        download_state.add_file(
                            file_id,
                            local_file_name,
                            final_file_size,
                            file_b3hash,
                            is_decrypting,
                        );

                        if let Err(e) = download_state.save(state_path).await {
                            eprintln!("  ⚠ Failed to save state: {:#}", e);
                        }
                    }
                }
                Err(e) => {
                    // Clean up partial temporary file on failure
                    let _ = tokio::fs::remove_file(&temp_file_path).await;
                    active_progress.file_finished(&file_pb, &filename, false);
                    return Err(e.into());
                }
            }
        }

        if let Some(p) = &transfer_progress {
            p.all_finished();
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_file_url() {
        assert_eq!(
            parse_url_target("https://pixeldrain.com/u/abc123xyz").unwrap(),
            UrlTarget::File("abc123xyz".to_string())
        );
        assert_eq!(
            parse_url_target("https://pixeldrain.com/u/abc123xyz/").unwrap(),
            UrlTarget::File("abc123xyz".to_string())
        );
        assert_eq!(
            parse_url_target("https://pixeldrain.com/u/abc123xyz?embed").unwrap(),
            UrlTarget::File("abc123xyz".to_string())
        );
        assert_eq!(
            parse_url_target("pixeldrain.com/u/abc123xyz").unwrap(),
            UrlTarget::File("abc123xyz".to_string())
        );
        assert_eq!(
            parse_url_target("/u/abc123xyz").unwrap(),
            UrlTarget::File("abc123xyz".to_string())
        );
        assert_eq!(
            parse_url_target("abc123xyz").unwrap(),
            UrlTarget::Id("abc123xyz".to_string())
        );
    }

    #[test]
    fn test_parse_album_url() {
        assert_eq!(
            parse_url_target("https://pixeldrain.com/l/alb123xyz").unwrap(),
            UrlTarget::Album("alb123xyz".to_string())
        );
        assert_eq!(
            parse_url_target("https://pixeldrain.com/l/alb123xyz/").unwrap(),
            UrlTarget::Album("alb123xyz".to_string())
        );
        assert_eq!(
            parse_url_target("https://pixeldrain.com/list/alb123xyz").unwrap(),
            UrlTarget::Album("alb123xyz".to_string())
        );
        assert_eq!(
            parse_url_target("pixeldrain.com/l/alb123xyz").unwrap(),
            UrlTarget::Album("alb123xyz".to_string())
        );
        assert_eq!(
            parse_url_target("/l/alb123xyz").unwrap(),
            UrlTarget::Album("alb123xyz".to_string())
        );
        assert_eq!(
            parse_url_target("https://pixeldrain.com/l/alb123xyz?foo=bar").unwrap(),
            UrlTarget::Album("alb123xyz".to_string())
        );
    }

    #[test]
    fn test_invalid_url() {
        assert!(parse_url_target("https://example.com/other").is_err());
        assert!(parse_url_target("https://pixeldrain.com/l/").is_err());
        assert!(parse_url_target("https://pixeldrain.com/u/").is_err());
        assert!(parse_url_target("").is_err());
    }
}
