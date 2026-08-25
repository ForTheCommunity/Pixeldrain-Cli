use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use crate::auth;
use anyhow::{Result, anyhow};
use humansize::{DECIMAL, format_size};
use serde::{Deserialize, Serialize};
use tabled::{
    Tabled,
    settings::{Alignment, Style},
};

pub enum AlbumAction {}

impl AlbumAction {
    pub async fn list_all() -> Result<()> {
        let api_key = auth::get_api_key()?;

        let response = reqwest::Client::new()
            .get("https://pixeldrain.com/api/user/lists")
            .basic_auth("", Some(api_key))
            .send()
            .await?;

        if response.status().is_success() {
            let res_data: AlbumListResponse = response.json().await?;

            if res_data.lists.is_empty() {
                println!("No Albums Found....");
                return Ok(());
            }

            // showing data in table,
            let mut table = tabled::Table::new(res_data.lists);
            let table_style = Style::modern();
            let alignment = Alignment::center();
            table.with(table_style).with(alignment);
            println!("{table}")
        } else {
            println!(
                "  Failed to fetch albums, Status Code : {}",
                response.status()
            )
        }

        Ok(())
    }

    pub async fn show_all_files(res_data: AlbumDetailResponse) {
        // showing data in table,
        let rows: Vec<TableFileRow> = res_data
            .files
            .into_iter()
            .map(|file| TableFileRow {
                id: file.id,
                name: file.name,
                size: format_size(file.size, DECIMAL),
                date_upload: file.date_upload,
                mime_type: file.mime_type,
            })
            .collect();

        // Render table using the presentation struct
        let mut table = tabled::Table::new(rows);
        let table_style = Style::modern();
        let alignment = Alignment::center();
        table.with(table_style).with(alignment);
        println!("{table}");
    }

    pub async fn all_files(album_id: &str) -> Result<AlbumDetailResponse> {
        let api_key = auth::get_api_key()?;
        let response = reqwest::Client::new()
            .get(format!("https://pixeldrain.com/api/list/{}", album_id))
            .basic_auth("", Some(api_key))
            .send()
            .await?;
        if response.status().is_success() {
            let res_data: AlbumDetailResponse = response.json().await?;

            if res_data.files.is_empty() {
                let msg = "  Albumn has 0 files. which isn't possible, it can be a bug.";
                println!("{msg}");
                return Err(anyhow!(msg));
            }

            Ok(res_data)
        } else {
            let msg = format!(
                "  Failed to fetch albums, Status Code : {}",
                response.status()
            );
            println!("{msg}");
            Err(anyhow!(msg))
        }
    }

    pub async fn delete(album_id: &str) -> Result<()> {
        let api_key = auth::get_api_key()?;
        Self::delete_with_key(album_id, &api_key).await?;
        Ok(())
    }

    pub async fn hard_delete(album_id: &str) -> Result<()> {
        let api_key = auth::get_api_key()?;
        let http_c = reqwest::Client::new();

        println!("  Fetching Files...");

        let response = http_c
            .get(format!("https://pixeldrain.com/api/list/{}", album_id))
            .basic_auth("", Some(api_key.clone()))
            .send()
            .await?;

        if response.status().is_success() {
            let res_data: AlbumDetailResponse = response.json().await?;

            let total_files = res_data.files.len();
            if total_files == 0 {
                println!("  Album has 0 files. Which isn't possible, it can be a bug.");
                return Ok(());
            }

            // first : deleting album itself.
            Self::delete_with_key(album_id, &api_key).await?;

            // Shared thread-safe counter for deleted files
            let deleted_count = Arc::new(AtomicUsize::new(0));
            let mut set = tokio::task::JoinSet::new();

            for a_file in res_data.files {
                let client = http_c.clone();
                let api_key = api_key.clone();
                let deleted_count = Arc::clone(&deleted_count);

                set.spawn(async move {
                    let url = format!("https://pixeldrain.com/api/file/{}", a_file.id);
                    let res = client
                        .delete(&url)
                        .basic_auth("", Some(api_key))
                        .send()
                        .await;

                    match res {
                        Ok(r) if r.status().is_success() => {
                            // Increment and fetch the current count atomically
                            let current = deleted_count.fetch_add(1, Ordering::SeqCst) + 1;
                            print!("\r  Deleted: {}/{} files", current, total_files);
                            let _ = std::io::Write::flush(&mut std::io::stdout());
                        }
                        Ok(r) => {
                            println!(
                                "\n  Failed to delete file {}: Status {}",
                                a_file.id,
                                r.status()
                            );
                        }
                        Err(e) => {
                            println!("\n    Error deleting file {}: {}", a_file.id, e);
                        }
                    }
                });
            }

            // Await all tasks to complete
            while let Some(_res) = set.join_next().await {}
            println!("\n  Finished deleting album contents.");
        } else {
            println!(
                "  Failed to fetch albums, Status Code : {}",
                response.status()
            );
        }

        Ok(())
    }

    async fn delete_with_key(album_id: &str, api_key: &str) -> Result<()> {
        let http_c = reqwest::Client::new();

        println!("  Deleting album....");

        let response = http_c
            .delete(format!("https://pixeldrain.com/api/list/{}", album_id))
            .basic_auth("", Some(api_key.to_string()))
            .send()
            .await?;

        if response.status().is_success() {
            println!("  Album Deleted Successfully.");
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

                eprintln!("  Error deleting album. [{}]: {}", value, message);
            } else {
                eprintln!("  Error deleting album: {}", text);
            }
        }

        Ok(())
    }

    pub async fn add_files_to_album(
        http_c: &reqwest::Client,
        api_key: &str,
        album_id: &str,
        new_file_ids: &[String],
    ) -> Result<()> {
        if new_file_ids.is_empty() {
            return Err(anyhow::anyhow!(
                "No successfully uploaded files to add to album"
            ));
        }

        let url = format!("https://pixeldrain.com/api/list/{}", album_id);

        // Getting existing album
        let response = http_c
            .get(&url)
            .basic_auth("", Some(api_key))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();

            return Err(anyhow::anyhow!(
                "Failed to get album '{}': HTTP {}: {}",
                album_id,
                status,
                body
            ));
        }

        let album: AlbumDetailResponse = response.json().await?;

        // Building complete file list
        let mut files = album
            .files
            .into_iter()
            .map(|file| {
                serde_json::json!({
                    "id": file.id
                })
            })
            .collect::<Vec<_>>();

        for file_id in new_file_ids {
            files.push(serde_json::json!({
                "id": file_id
            }));
        }

        // Updating Album
        let body = serde_json::json!({
            "title": album.title,
            "files": files
        });

        let response = http_c
            .put(&url)
            .basic_auth("", Some(api_key))
            .json(&body)
            .send()
            .await?;

        let status = response.status();

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();

            return Err(anyhow::anyhow!(
                "Failed to update album '{}': HTTP {}: {}",
                album_id,
                status,
                body
            ));
        }

        println!(
            "  ✓ Added {} file(s) to album '{}'.",
            new_file_ids.len(),
            album.title
        );

        println!("  https://pixeldrain.com/l/{}", album_id);

        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AlbumListResponse {
    pub lists: Vec<Album>,
}

#[derive(Serialize, Deserialize, Debug, Tabled)]
pub struct Album {
    #[tabled(rename = "ID", order = 0)]
    pub id: String,
    #[tabled(rename = "Title", order = 1)]
    pub title: String,
    #[tabled(rename = "Created At", order = 4)]
    pub date_created: String,
    #[tabled(rename = "File Count", order = 2)]
    pub file_count: usize,
    #[tabled(rename = "Write Permission", order = 3)]
    pub can_edit: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AlbumDetailResponse {
    pub success: bool,
    pub id: String,
    pub title: String,
    pub date_created: String,
    pub file_count: usize,
    pub can_edit: bool,
    pub files: Vec<AlbumFile>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AlbumFile {
    pub detail_href: String,

    pub description: String,

    pub id: String,

    pub name: String,

    pub size: u64,

    pub views: u64,

    pub bandwidth_used: u64,

    pub bandwidth_used_paid: u64,

    pub downloads: u64,

    pub date_upload: String,

    pub date_last_view: String,

    pub mime_type: String,

    pub thumbnail_href: String,

    pub hash_sha256: String,

    pub delete_after_date: String,

    pub delete_after_downloads: u64,

    pub availability: String,

    pub availability_message: String,

    pub abuse_type: String,

    pub abuse_reporter_name: String,

    pub can_edit: bool,

    pub can_download: bool,

    pub show_ads: bool,

    pub allow_video_player: bool,

    pub download_speed_limit: u64,

    // stop deserializing
    // new fields may added in api in future
    #[serde(skip_deserializing)]
    pub _extra: (),
}

#[derive(Tabled)]
pub struct TableFileRow {
    #[tabled(rename = "File ID", order = 0)]
    pub id: String,

    #[tabled(rename = "File Name", order = 1)]
    pub name: String,

    #[tabled(rename = "Size", order = 2)]
    pub size: String,

    #[tabled(rename = "Uploaded At", order = 3)]
    pub date_upload: String,

    #[tabled(rename = "File Type", order = 4)]
    pub mime_type: String,
}
