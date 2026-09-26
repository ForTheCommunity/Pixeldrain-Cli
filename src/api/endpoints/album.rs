use std::sync::{Arc, atomic::AtomicUsize};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::api::models::album::{AlbumDetailResponse, AlbumListResponse};

pub struct AlbumApi<'a> {
    pub api_key: &'a str,
    pub id: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub enum AlbumDeleteEvent {
    FileDeleted { current: usize, total: usize },
    FileFailed { file_id: String, error: String },
}

const API_ENDPOINT: &str = "https://pixeldrain.com/api";

impl<'a> AlbumApi<'a> {
    pub async fn get_all(&self) -> Result<AlbumListResponse> {
        let url = format!("{}/user/lists", API_ENDPOINT);
        let response = reqwest::Client::new()
            .get(url)
            .basic_auth("", Some(self.api_key))
            .send()
            .await?;

        if response.status().is_success() {
            let response_data: AlbumListResponse = response.json().await?;

            Ok(response_data)
        } else {
            Err(anyhow!(response.status()))
        }
    }

    pub async fn get_files(&self) -> Result<AlbumDetailResponse> {
        let url = format!("{}/list/{}", API_ENDPOINT, self.id.unwrap());
        let response = reqwest::Client::new()
            .get(url)
            .basic_auth("", Some(self.api_key))
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json::<AlbumDetailResponse>().await?)
        } else {
            Err(anyhow!(response.status()))
        }
    }

    pub async fn delete_album(&self) -> Result<()> {
        let url = format!("{}/list/{}", API_ENDPOINT, self.id.unwrap());

        let response = reqwest::Client::new()
            .delete(url)
            .basic_auth("", Some(self.api_key))
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            let text = response.text().await.unwrap_or_default();

            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                let message = json
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&text);
                let value = json
                    .get("value")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");

                Err(anyhow!("  Error deleting album. [{}]: {}", value, message))
            } else {
                Err(anyhow!("  Error deleting album: {}", text))
            }
        }
    }

    pub async fn hard_delete(&self, tx: Option<mpsc::Sender<AlbumDeleteEvent>>) -> Result<()> {
        // featching files of a album
        let album_details = Self::get_files(&self).await?;

        // Deleting Album
        Self::delete_album(&self).await?;

        // Deleting all files
        // Shared thread-safe counter for deleted files
        let deleted_count = Arc::new(AtomicUsize::new(0));
        let total_files = album_details.files.len();

        let mut set = tokio::task::JoinSet::new();

        let http_c = reqwest::Client::new();

        for a_file in album_details.files {
            let http_c = http_c.clone();
            let api_key = self.api_key.to_owned();
            let deleted_count = Arc::clone(&deleted_count);
            let tx = tx.clone();
            set.spawn(async move {
                let url = format!("{}/file/{}", API_ENDPOINT, a_file.id);
                let response = http_c
                    .delete(&url)
                    .basic_auth("", Some(api_key))
                    .send()
                    .await;

                if let Some(tx) = tx {
                    match response {
                        Ok(r) if r.status().is_success() => {
                            let current =
                                deleted_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                            let _ = tx
                                .send(AlbumDeleteEvent::FileDeleted {
                                    current,
                                    total: total_files,
                                })
                                .await;
                        }
                        Ok(r) => {
                            let _ = tx
                                .send(AlbumDeleteEvent::FileFailed {
                                    file_id: a_file.id,
                                    error: format!("Status {}", r.status()),
                                })
                                .await;
                        }
                        Err(e) => {
                            let _ = tx
                                .send(AlbumDeleteEvent::FileFailed {
                                    file_id: a_file.id,
                                    error: e.to_string(),
                                })
                                .await;
                        }
                    }
                }
            });
        }
        drop(tx);
        // Await all background deletion tasks
        while let Some(_res) = set.join_next().await {}
        Ok(())
    }

    // Creates an album multiple files in a single batch request.
    pub async fn create_album(&self, album_name: &str, file_ids: &[String]) -> Result<String> {
        let files = file_ids
            .iter()
            .map(|id| json!({"id": id}))
            .collect::<Vec<Value>>();

        let payload = serde_json::json!({
            "title": album_name,
            "files": files,
        });

        let response = reqwest::Client::new()
            .post(format!("{}/list", API_ENDPOINT))
            .basic_auth("", Some(self.api_key))
            .json(&payload)
            .send()
            .await?;

        let status = response.status();
        let body = response
            .text()
            .await
            .context("Failed to read album creation response")?;

        if !status.is_success() {
            bail!("Failed to create album: HTTP {}\n{}", status, body);
        }

        let response_json: serde_json::Value =
            serde_json::from_str(&body).context("Invalid album creation response")?;

        let album_id = response_json
            .get("id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| anyhow::anyhow!("Album response did not contain an ID"))?;

        Ok(album_id.to_owned())
    }

    // Fetches the existing list, appends new file IDs,
    // and updates the remote album in a single call.
    pub async fn add_file(self, file_ids: &[String]) -> Result<()> {
        // update album:
        // 1. fetching files of a album.
        // 2. old files ids + new file id
        // 3. update remote album

        let old_album_data = self.get_files().await?;

        // Building complete file list
        let mut files = old_album_data
            .files
            .into_iter()
            .map(|file| {
                json!({
                    "id": file.id
                })
            })
            .collect::<Vec<Value>>();

        // appending new files
        for id in file_ids {
            files.push(json!({"id": id}));
        }

        // Updating Album
        let body = serde_json::json!({
            "title": &old_album_data.title,
            "files": files
        });

        let album_id = self
            .id
            .ok_or_else(|| anyhow!("Album ID is not provided..."))?;

        let response = reqwest::Client::new()
            .put(format!("{}/list/{}", API_ENDPOINT, album_id))
            .basic_auth("", Some(self.api_key))
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();

            return Err(anyhow::anyhow!(
                "Failed to update album '{}': HTTP {}: {}",
                self.id.unwrap_or("NONE_ALBUM_ID"),
                status,
                body
            ));
        }

        Ok(())
    }
}
