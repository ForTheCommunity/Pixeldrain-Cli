use anyhow::Result;
use tokio::{
    fs,
    sync::{Mutex, Semaphore},
    task::JoinSet,
};

use crate::{
    api::{client::ApiClient, models::upload::UploadResponse},
    app::album::AlbumAction,
    core::{
        collector::collect_files,
        format_bytes,
        progress::{ProgressReader, TransferProgress, TransferType},
        state::upload::UploadState,
    },
};
use std::{path::PathBuf, sync::Arc};

#[derive(Debug)]
pub struct UploadPipelineOptions {
    pub paths: Vec<PathBuf>,
    pub album_name: Option<Vec<String>>,
    pub album_id: Option<String>,
    pub formats: Option<Vec<String>>,
    pub delete_after: bool,
    pub state: Option<PathBuf>,
    pub max_concurrent_small_uploads: usize,
}

pub struct UploadPipeline {
    client: ApiClient,
    options: UploadPipelineOptions,
}

impl Default for UploadPipelineOptions {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            album_name: None,
            album_id: None,
            formats: None,
            delete_after: false,
            state: None,
            max_concurrent_small_uploads: 10,
        }
    }
}

impl UploadPipeline {
    pub fn new(client: ApiClient, options: UploadPipelineOptions) -> Self {
        Self { client, options }
    }

    pub async fn run(self) -> Result<()> {
        let mut album_name_cli = String::new();
        if let Some(al_name) = &self.options.album_name {
            album_name_cli = al_name.join(" ");
            println!("Album Name : {}", album_name_cli);
        };

        // scanning files & filtering files,
        let all_files = collect_files(&self.options.paths, self.options.formats.as_deref())?;

        if all_files.is_empty() {
            if let Some(filters) = &self.options.formats {
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
                println!("  No files found with format : {formats}",);
                return Ok(());
            } else {
                println!("  No files found in given path...");
                return Ok(());
            }
        }

        // Load or create upload state
        let upload_state = match &self.options.state {
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

        // grouping files by threshold.
        const SMALL_FILE_THRESHOLD: u64 = 20 * 1024 * 1024; // 20MB

        let mut small_files: Vec<(PathBuf, u64)> = Vec::new();
        let mut large_files: Vec<(PathBuf, u64)> = Vec::new();
        let mut skipped_file_ids: Vec<String> = Vec::new();

        for a_file in all_files {
            let metadata = match fs::metadata(&a_file).await {
                Ok(metadata) => metadata,
                Err(e) => {
                    eprintln!(
                        "  ⚠ Failed to read metadata for {}: {}",
                        a_file.display(),
                        e
                    );
                    continue;
                }
            };

            let file_size = metadata.len();

            // skip files that were already uploaded with the same size.
            if let Some(existing_id) = upload_state.get_file_id(&a_file, file_size) {
                skipped_file_ids.push(existing_id.to_string());
                continue;
            }

            if file_size <= SMALL_FILE_THRESHOLD {
                small_files.push((a_file, file_size));
            } else {
                large_files.push((a_file, file_size));
            }
        }

        if !skipped_file_ids.is_empty() {
            println!(
                "  ✦ Skipped {} already-uploaded file(s) (from state)",
                skipped_file_ids.len()
            );
        }

        let (total_files, total_transfer_size) = (
            small_files.len() + large_files.len(),
            small_files.iter().map(|(_, size)| size).sum::<u64>()
                + large_files.iter().map(|(_, size)| size).sum::<u64>(),
        );

        let upload_state = Arc::new(Mutex::new(upload_state));
        let state_path = self.options.state.clone();

        // Pre-initialize album_state: prefer CLI flag, then fall back to state file.
        let album_id = match &self.options.album_id {
            Some(id) => Some(id.clone()),
            None => upload_state.lock().await.get_album_id().map(String::from),
        };

        let album_state: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(album_id));

        // Check if we are actively managing an album (via CLI flag OR loaded state ID)
        let has_album = self.options.album_name.is_some()
            || self.options.album_id.is_some()
            || album_state.lock().await.is_some();

        // api client
        let api_client = &self.client;
        let api_client: Arc<ApiClient> = Arc::new(api_client.clone());
        // Target album name or fallback string when adding to existing ID
        let target_album_name = &album_name_cli;

        if total_files == 0 {
            if has_album {
                if let Some(album_id) = album_state.lock().await.as_deref() {
                    println!("Album URL: https://pixeldrain.com/l/{}", album_id);
                }
            }
            return Ok(());
        }

        println!(
            "   ✦ Files to upload : {} \n   ✦ Upload size     : {}",
            total_files,
            format_bytes(total_transfer_size)
        );

        let progress =
            TransferProgress::new(TransferType::Upload, total_files, total_transfer_size);

        let mut all_responses: Vec<UploadResponse> = Vec::new();

        // uploading files...
        // Processing small files.
        if !small_files.is_empty() {
            let responses = self
                .process_sfs(
                    small_files,
                    api_client.clone(),
                    &progress,
                    upload_state.clone(),
                    state_path.clone(),
                )
                .await?;

            // Batch album create/update (includes skipped IDs on first batch).
            if has_album {
                if let Some(api_key) = api_client.api_key.as_deref() {
                    let batch_ids: Vec<String> = responses.iter().map(|r| r.id.clone()).collect();

                    if !batch_ids.is_empty() {
                        if let Err(err) = AlbumAction::update_album(
                            target_album_name,
                            api_key,
                            &album_state,
                            &batch_ids,
                        )
                        .await
                        {
                            progress.overall_pb.println(format!(
                                "  ⚠ Failed to update album for small files: {:#}",
                                err
                            ));
                        }
                    }
                }
            }

            all_responses.extend(responses);
        }

        // Processing large files.
        if !large_files.is_empty() {
            let responses = self
                .process_lfs(
                    large_files,
                    api_client,
                    &progress,
                    &album_state,
                    target_album_name,
                    upload_state.clone(),
                    state_path.clone(),
                    has_album,
                )
                .await?;
            all_responses.extend(responses);
        }

        // Persist final album ID into state.
        if let Some(album_id) = album_state.lock().await.as_ref() {
            let album_url = format!("https://pixeldrain.com/l/{}", album_id);
            progress
                .overall_pb
                .println(format!("\n  ✦ Album URL: {}", album_url));

            upload_state.lock().await.set_album_id(album_id.clone());
        }

        // Final state save.
        if let Some(state_path) = &state_path {
            if let Err(e) = upload_state.lock().await.save(state_path).await {
                eprintln!("  ⚠ Failed to save final state: {:#}", e);
            }
        }

        // all transfer done.
        progress.all_finished();

        Ok(())
    }

    /// Saves upload state to disk if a state path is configured.
    async fn save_state(upload_state: &Arc<Mutex<UploadState>>, state_path: &Option<PathBuf>) {
        if let Some(path) = state_path {
            if let Err(e) = upload_state.lock().await.save(path).await {
                eprintln!("  ⚠ Failed to save state: {:#}", e);
            }
        }
    }

    // uploads small files concurrently.
    async fn process_sfs(
        &self,
        files: Vec<(PathBuf, u64)>,
        api_client: Arc<ApiClient>,
        progress: &TransferProgress,
        upload_state: Arc<Mutex<UploadState>>,
        state_path: Option<PathBuf>,
    ) -> Result<Vec<UploadResponse>> {
        let semaphore = Arc::new(Semaphore::new(self.options.max_concurrent_small_uploads));
        let mut join_set = JoinSet::new();
        let delete_after = self.options.delete_after;

        for (path, size) in files {
            let permit = semaphore.clone().acquire_owned().await?;
            let api_client = api_client.clone();
            let progress = progress.clone();
            let upload_state = upload_state.clone();
            let state_path = state_path.clone();

            join_set.spawn(async move {
                let _permit = permit; // hold a slot until task finishes

                let file_name = path.file_name().unwrap().to_string_lossy().to_string();
                let file_pb = progress.file_started(&file_name, size);

                let file = match fs::File::open(&path).await {
                    Ok(f) => f,
                    Err(e) => {
                        progress.overall_pb.println(format!(
                            "  ⚠ Failed to open file {}: {}",
                            path.display(),
                            e
                        ));
                        progress.file_finished_with_url(&file_pb, &file_name, false, None);
                        return Err(e.into());
                    }
                };

                let reader = ProgressReader::new(
                    file,
                    file_pb.clone(),
                    progress.overall_pb.clone(),
                    !progress.is_single_file(),
                );

                let result = api_client
                    .upload_file_stream(&file_name, reader, size)
                    .await;

                match &result {
                    Ok(res) => {
                        let url = format!("https://pixeldrain.com/u/{}", res.id);
                        progress.file_finished_with_url(&file_pb, &file_name, true, Some(&url));

                        // Record uploaded file into state.
                        upload_state
                            .lock()
                            .await
                            .add_file(path.clone(), res.id.clone(), size);
                        Self::save_state(&upload_state, &state_path).await;

                        // Delete local file if -d flag was specified
                        if delete_after {
                            match fs::remove_file(&path).await {
                                Ok(_) => {
                                    progress.overall_pb.println(format!(
                                        "  🗑 Deleted local file: {}",
                                        path.display()
                                    ));
                                }
                                Err(e) => {
                                    progress.overall_pb.println(format!(
                                        "  ⚠ Failed to delete local file {}: {}",
                                        path.display(),
                                        e
                                    ));
                                }
                            }
                        }
                    }
                    Err(_) => {
                        progress.file_finished_with_url(&file_pb, &file_name, false, None);
                    }
                }

                Ok::<UploadResponse, anyhow::Error>(result?)
            });
        }

        let mut results = Vec::new();

        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(Ok(response)) => results.push(response),
                Ok(Err(err)) => {
                    progress
                        .overall_pb
                        .println(format!("  Upload failed: {:#}", err));
                }
                Err(err) => {
                    progress
                        .overall_pb
                        .println(format!("  Upload Task failed: {:#}", err));
                }
            }
        }

        Ok(results)
    }

    // uploads large files sequentially
    async fn process_lfs(
        &self,
        files: Vec<(PathBuf, u64)>,
        api_client: Arc<ApiClient>,
        progress: &TransferProgress,
        album_state: &Arc<Mutex<Option<String>>>,
        album_name: &str,
        upload_state: Arc<Mutex<UploadState>>,
        state_path: Option<PathBuf>,
        has_album: bool,
    ) -> Result<Vec<UploadResponse>> {
        let mut results = Vec::new();

        for (path, size) in files {
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_string();

            let file_pb = progress.file_started(&file_name, size);

            let file = fs::File::open(&path).await?;
            let reader = ProgressReader::new(
                file,
                file_pb.clone(),
                progress.overall_pb.clone(),
                !progress.is_single_file(),
            );

            match api_client
                .upload_file_stream(&file_name, reader, size)
                .await
            {
                Ok(res) => {
                    let url = format!("https://pixeldrain.com/u/{}", res.id);
                    progress.file_finished_with_url(&file_pb, &file_name, true, Some(&url));

                    // Record uploaded file into state.
                    upload_state
                        .lock()
                        .await
                        .add_file(path.clone(), res.id.clone(), size);
                    Self::save_state(&upload_state, &state_path).await;

                    // Sequential album update for single large file
                    if has_album {
                        if let Some(api_key) = api_client.api_key.as_deref() {
                            if let Err(err) = AlbumAction::update_album(
                                album_name,
                                api_key,
                                album_state,
                                &[res.id.clone()],
                            )
                            .await
                            {
                                progress.overall_pb.println(format!(
                                    "  ⚠ Failed to add file {} to album: {:#}",
                                    res.id, err
                                ));
                            }
                        }
                    }

                    // Delete local file if -d flag was specified
                    if self.options.delete_after {
                        match fs::remove_file(&path).await {
                            Ok(_) => {
                                progress
                                    .overall_pb
                                    .println(format!("  🗑 Deleted local file: {}", path.display()));
                            }
                            Err(e) => {
                                progress.overall_pb.println(format!(
                                    "  ⚠ Failed to delete local file {}: {}",
                                    path.display(),
                                    e
                                ));
                            }
                        }
                    }

                    results.push(res);
                }
                Err(e) => {
                    progress.file_finished_with_url(&file_pb, &file_name, false, None);
                    progress.overall_pb.println(format!("  Error -> {}", e));
                    continue;
                }
            }
        }
        Ok(results)
    }
}
