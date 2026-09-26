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
    },
};
use std::{path::PathBuf, sync::Arc};

#[derive(Debug)]
pub struct UploadPipelineOptions {
    pub paths: Vec<PathBuf>,
    pub album_name: Option<String>,
    pub album_id: Option<String>,
    pub formats: Option<Vec<String>>,
    pub delete_after: bool,
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
            max_concurrent_small_uploads: 10,
        }
    }
}

impl UploadPipeline {
    pub fn new(client: ApiClient, options: UploadPipelineOptions) -> Self {
        Self { client, options }
    }

    pub async fn run(self) -> Result<()> {
        if let Some(al_name) = &self.options.album_name {
            println!("Album Name : {}", al_name);
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

        // Hash & State Check [TODO]

        // grouping files by threshold.
        const SMALL_FILE_THRESHOLD: u64 = 20 * 1024 * 1024; // 20MB

        let mut small_files: Vec<(PathBuf, u64)> = Vec::new();
        let mut large_files: Vec<(PathBuf, u64)> = Vec::new();

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

            if file_size < SMALL_FILE_THRESHOLD {
                small_files.push((a_file, file_size));
            } else if file_size > SMALL_FILE_THRESHOLD {
                large_files.push((a_file, file_size));
            } else {
                eprintln!(
                    "  ⚠ Failed to group files. got error in file : {} , size : {}",
                    a_file.display(),
                    metadata.len()
                );
            }
        }

        let (total_files, total_transfer_size) = (
            small_files.len() + large_files.len(),
            small_files.iter().map(|(_, size)| size).sum::<u64>()
                + large_files.iter().map(|(_, size)| size).sum::<u64>(),
        );
        println!(
            "   ✦ Files to upload : {} \n   ✦ Upload size     : {}",
            total_files,
            format_bytes(total_transfer_size)
        );

        // Total Counts for progress tracking..
        let total_files = small_files.len() + large_files.len();
        let total_bytes: u64 = small_files.iter().map(|(_, size)| size).sum::<u64>()
            + large_files.iter().map(|(_, size)| size).sum::<u64>();

        let progress = TransferProgress::new(TransferType::Upload, total_files, total_bytes);

        // api client
        let api_client = &self.client;
        let api_client: Arc<ApiClient> = Arc::new(api_client.clone());

        // Pre-initialize album_state with the existing album_id if provided via -i
        let album_state: Arc<Mutex<Option<String>>> =
            Arc::new(Mutex::new(self.options.album_id.clone()));

        let mut all_responses: Vec<UploadResponse> = Vec::new();

        // Target album name or fallback string when adding to existing ID
        let target_album_name = self
            .options
            .album_name
            .as_deref()
            .unwrap_or("Existing Album");

        // uploading files...
        // Processing small files.
        if !small_files.is_empty() {
            let responses = self
                .process_sfs(small_files, api_client.clone(), &progress)
                .await?;

            // Batch album create/update.
            if self.options.album_name.is_some() || self.options.album_id.is_some() {
                // if let Some(album_name) = &self.options.album_name {
                if let Some(api_key) = api_client.api_key.as_deref() {
                    let small_file_ids = responses
                        .iter()
                        .map(|r| r.id.clone())
                        .collect::<Vec<String>>();

                    if let Err(err) = AlbumAction::update_album(
                        target_album_name,
                        api_key,
                        &album_state,
                        &small_file_ids,
                    )
                    .await
                    {
                        progress.overall_pb.println(format!(
                            "  ⚠ Failed to update album for small files: {:#}",
                            err
                        ));
                    }
                }
            };

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
                )
                .await?;
            all_responses.extend(responses);
        }

        // Print final Album URL if an album was created or updated
        if let Some(album_id) = album_state.lock().await.as_ref() {
            let album_url = format!("https://pixeldrain.com/l/{}", album_id);
            progress
                .overall_pb
                .println(format!("\n  ✦ Album URL: {}", album_url));
        }

        // all transfer done.
        progress.all_finished();

        Ok(())
    }

    // uploads small files concurrently.
    async fn process_sfs(
        &self,
        files: Vec<(PathBuf, u64)>,
        api_client: Arc<ApiClient>,
        progress: &TransferProgress,
    ) -> Result<Vec<UploadResponse>> {
        let semaphore = Arc::new(Semaphore::new(self.options.max_concurrent_small_uploads));
        let mut join_set = JoinSet::new();

        for (path, size) in files {
            let permit = semaphore.clone().acquire_owned().await?;
            let api_client = api_client.clone();
            let progress = progress.clone();

            join_set.spawn(async move {
                let _permit = permit; // hold a slot until task finishes

                let file_name = path.file_name().unwrap().to_string_lossy().to_string();
                let file_pb = progress.file_started(&file_name, size);

                let file = fs::File::open(&path).await.unwrap();
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

                    // Sequential album update for single large file
                    if self.options.album_name.is_some() || self.options.album_id.is_some() {
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
