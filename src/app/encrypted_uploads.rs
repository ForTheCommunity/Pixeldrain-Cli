use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Result, anyhow};
use indicatif::{ProgressBar, ProgressStyle};
use tokio::fs::{self, File};
use transformer::crypt::{check_master_password, encrypt_file_ks, encrypt_file_wp};

use crate::core::hash::{compute_b3hash, compute_b3hash_with_pb};
use crate::core::password::{resolve_secret, resolve_secret_confirmed};
use crate::{
    api::endpoints::album::AlbumApi,
    app::uploader::UploadPipeline,
    core::{
        collector::collect_files,
        exceeds_max_upload_size, format_bytes,
        progress::{ProgressReader, TransferProgress, TransferType},
        state::upload::enc_state::EncUploadState,
        storage::project_data_dir,
    },
};

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

    let chunk_size = (upload_options.chunk_size as usize) * 1024 * 1024;
    let mut files_to_upload = Vec::new();
    let mut skipped_files = Vec::new();

    for file in all_files {
        let metadata = match fs::metadata(&file).await {
            Ok(metadata) => metadata,
            Err(e) => {
                eprintln!("  ⚠ Failed to read metadata for {}: {}", file.display(), e);
                continue;
            }
        };

        let file_size = metadata.len();
        if exceeds_max_upload_size(file_size) {
            eprintln!(
                "  ⚠ Skipping {} ({}): exceeds 10 GiB limit",
                file.display(),
                format_bytes(file_size)
            );
            continue;
        }

        let original_filename = file.file_name().unwrap().to_str().unwrap().to_owned();

        if upload_state
            .file_lookup(&original_filename, file_size, &file, chunk_size)
            .await?
        {
            skipped_files.push(original_filename);
            continue;
        }

        files_to_upload.push((file, original_filename, file_size));
    }

    if !skipped_files.is_empty() {
        println!(
            "  ✦ Skipped {} already-uploaded file(s) (from state)",
            skipped_files.len()
        );
    }

    let total_files = files_to_upload.len();
    let total_transfer_size: u64 = files_to_upload.iter().map(|(_, _, size)| *size).sum();

    if total_files == 0 {
        if let Some(ref album_id) = upload_state.album_id {
            println!(
                " Album ID: {} | URL -> https://pixeldrain.com/l/{}",
                album_id, album_id
            );
        }
        return Ok(());
    }

    println!(
        "   ✦ Files to upload : {} \n   ✦ Upload size     : {}",
        total_files,
        format_bytes(total_transfer_size)
    );
    println!("{}", transformer_notice());

    let ks_master_password: Option<String> =
        if let Some(keystore_path) = &upload_options.enc_keystore {
            let pwd = resolve_secret(
                upload_options.enc_keystore_pass.as_deref(),
                "PIXELDRAIN_KEYSTORE_PASS",
                " Enter Master Password for Keystore -> ",
            )?;
            match check_master_password(keystore_path, &pwd) {
                Ok(true) => Some(pwd),
                Ok(false) => {
                    eprintln!(" Enter correct Master Password for keystore !!!");
                    return Err(anyhow!("Incorrect master password"));
                }
                Err(e) => return Err(anyhow!("Error -> {}", e)),
            }
        } else {
            None
        };

    let enc_password: Option<String> = if let Some(opt_pwd) = &upload_options.enc_password {
        Some(resolve_secret_confirmed(
            opt_pwd.as_deref(),
            "PIXELDRAIN_ENCRYPT_PASS",
            " Enter Encryption Password -> ",
            " Confirm Encryption Password -> ",
            " Passwords do not match!",
        )?)
    } else {
        None
    };

    let is_multi = total_files > 1;

    let progress = if is_multi {
        Some(TransferProgress::new(
            TransferType::Upload,
            total_files,
            total_transfer_size,
        ))
    } else {
        None
    };

    // Prepare temp directory once outside the loop
    let temp_dir = project_data_dir()?.join("temp_files");
    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir).await?;
    }
    fs::create_dir_all(&temp_dir).await?;

    // upload loop
    for (file, orignal_filename, file_size) in files_to_upload {
        // computing blake3 Hash of orignal file.
        let original_file_b3hash = if let Some(p) = &progress {
            compute_b3hash_with_pb(
                chunk_size,
                file_size,
                &file,
                Some((&p.multi_pb, &p.overall_pb)),
            )
            .await?
        } else {
            compute_b3hash(chunk_size, file_size, &file).await?
        };

        // Setup Progress Bar for Encryption
        let enc_pb = if let Some(p) = &progress {
            p.multi_pb
                .insert_before(&p.overall_pb, ProgressBar::new(file_size))
        } else {
            ProgressBar::new(file_size)
        };
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

            encrypt_file_wp(
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
                if let Some(p) = &progress {
                    p.multi_pb.remove(&enc_pb);
                }
                path
            }
            Err(e) => {
                enc_pb.abandon_with_message("Encryption failed");
                if let Some(p) = &progress {
                    p.multi_pb.remove(&enc_pb);
                }
                return Err(e);
            }
        };

        let file_reader = File::open(&encrypted_file).await?;

        // Get the length of the *encrypted* file for streaming & progress bar total
        let encrypted_metadata = fs::metadata(&encrypted_file).await?;
        let encrypted_file_length = encrypted_metadata.len();

        if exceeds_max_upload_size(encrypted_file_length) {
            let _ = fs::remove_file(&encrypted_file).await;
            let msg = format!(
                "  ⚠ Skipping {} ({} after encryption): exceeds 10 GiB limit",
                orignal_filename,
                format_bytes(encrypted_file_length)
            );
            if let Some(p) = &progress {
                p.overall_pb.println(msg);
            } else {
                eprintln!("{}", msg);
            }
            continue;
        }

        let encrypted_file_name = encrypted_file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("encrypted_file");

        let single_progress;
        let active_progress = match &progress {
            Some(p) => p,
            None => {
                single_progress =
                    TransferProgress::new(TransferType::Upload, 1, encrypted_file_length);
                &single_progress
            }
        };

        let file_pb = active_progress.file_started(&orignal_filename, encrypted_file_length);

        // Wrap reader with progress bar tracker
        let progress_reader = ProgressReader::new(
            file_reader,
            file_pb.clone(),
            active_progress.overall_pb.clone(),
            !active_progress.is_single_file(),
        );

        let api_client = upload_pipeline.client.clone();

        let upload_response = match api_client
            .upload_file_stream(encrypted_file_name, progress_reader, encrypted_file_length)
            .await
        {
            Ok(resp) => {
                let _ = fs::remove_file(&encrypted_file).await;
                let url = format!("https://pixeldrain.com/u/{}", resp.id);
                active_progress.file_finished_with_url(
                    &file_pb,
                    &orignal_filename,
                    true,
                    Some(&url),
                );
                if !is_multi {
                    active_progress.all_finished();
                }
                resp
            }
            Err(e) => {
                active_progress.file_finished_with_url(&file_pb, &orignal_filename, false, None);
                return Err(e.into());
            }
        };

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
                            if let Some(p) = &progress {
                                p.overall_pb.println(format!(" Error -> {}", e.to_string()));
                            } else {
                                println!(" Error -> {}", e.to_string());
                            }
                        }
                    }
                }
            }
        }

        // deleting orignal file if delete flag is passed.
        if upload_options.delete_after {
            match fs::remove_file(&file).await {
                Ok(_) => {
                    if let Some(p) = &progress {
                        p.overall_pb
                            .println(format!("  🗑 Deleted orignal file: {}", file.display()));
                    } else {
                        println!("  🗑 Deleted orignal file: {}", file.display());
                    }
                }
                Err(e) => {
                    if let Some(p) = &progress {
                        p.overall_pb.println(format!(
                            "  ⚠ Failed to delete orignal file {}: {}",
                            file.display(),
                            e
                        ));
                    } else {
                        println!(
                            "  ⚠ Failed to delete orignal file {}: {}",
                            file.display(),
                            e
                        );
                    }
                }
            }
        }
    }

    // Print the current album ID and URL once it's created or updated
    if let Some(ref album_id) = upload_state.album_id {
        let album_url = format!("https://pixeldrain.com/l/{}", album_id);
        if let Some(p) = &progress {
            p.overall_pb
                .println(format!("\n  ✦ Album URL: {}", album_url));
        } else {
            println!("\n  ✦ Album URL: {}", album_url);
        }
    }

    if let Some(p) = &progress {
        p.all_finished();
    }

    Ok(())
}

const TRANSFORMER_GIT_URL: &str = "https://gitlab.com/ForTheCommunity/Transformer.git";

/// Returns the informational banner noting the external `transformer` dependency.
fn transformer_notice() -> String {
    let cyan = "\x1b[1;36m";
    let dim = "\x1b[90m";
    let reset = "\x1b[0m";

    format!(
        "\n   {cyan}┌───── Encryption (Transformer) ─────────────────────────────────────────────┐{reset}\n   \
           {cyan}│{reset}  pixeldrain-cli uses transformer for encrypting/decrypting files.          {cyan}│{reset}\n   \
           {cyan}│{reset}  {dim}For more info:{reset} {TRANSFORMER_GIT_URL}         {cyan}│{reset}\n   \
           {cyan}└────────────────────────────────────────────────────────────────────────────┘{reset}\n"
    )
}

async fn save_state(upload_state: &EncUploadState, state_path: &Option<PathBuf>) {
    if let Some(path) = state_path {
        if let Err(e) = upload_state.save(path).await {
            eprintln!("  ⚠ Failed to save state: {:#}", e);
        }
    }
}
