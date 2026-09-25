use std::io::Write;

use anyhow::{Ok, Result};
use tabled::{
    Tabled,
    settings::{Alignment, Style},
};
use tokio::sync::mpsc;

use crate::{
    api::endpoints::album::{AlbumApi, AlbumDeleteEvent},
    core::{format_bytes, storage::get_api_key},
};

pub struct AlbumAction {}

impl AlbumAction {
    pub async fn show_all() -> Result<()> {
        let api_key = get_api_key()?;
        let album_api = AlbumApi {
            api_key: api_key.as_str(),
            id: None,
        };
        let all_albums = AlbumApi::get_all(&album_api).await?;

        if all_albums.lists.is_empty() {
            println!("No Albums Found....");
            return Ok(());
        }

        // showing data in table,
        let mut table = tabled::Table::new(all_albums.lists);
        let table_style = Style::modern();
        let alignment = Alignment::center();
        table.with(table_style).with(alignment);
        println!("{table}");

        Ok(())
    }

    pub async fn list_album(id: String) -> Result<()> {
        let api_key = get_api_key()?;

        let album_api = AlbumApi {
            api_key: &api_key,
            id: Some(&id),
        };

        let all_files = AlbumApi::get_files(&album_api).await?;
        if all_files.files.is_empty() {
            println!("  Albumn has 0 files. which isn't possible, it can be a bug.");
            return Ok(());
        }

        // showing data in table,
        let rows: Vec<TableFileRow> = all_files
            .files
            .into_iter()
            .map(|file| TableFileRow {
                id: file.id,
                name: file.name,
                size: format_bytes(file.size),
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

        Ok(())
    }

    pub async fn delete(id: String) -> Result<()> {
        let api_key = get_api_key()?;

        let album_api = AlbumApi {
            api_key: &api_key,
            id: Some(&id),
        };
        AlbumApi::delete_album(&album_api).await?;
        println!("  Album Deleted Successfully.");
        Ok(())
    }

    pub async fn hard_delete(id: String) -> Result<()> {
        let api_key = get_api_key()?;

        let album_api = AlbumApi {
            api_key: &api_key,
            id: Some(&id),
        };

        let (tx, mut rx) = mpsc::channel::<AlbumDeleteEvent>(100);

        // consumer loop
        let ui_handle = tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                match event {
                    AlbumDeleteEvent::FileDeleted { current, total } => {
                        print!("\r  Deleted: {}/{} files", current, total);
                        let _ = std::io::stdout().flush();
                    }
                    AlbumDeleteEvent::FileFailed { file_id, error } => {
                        println!("\n  Failed to delete file {}: {}", file_id, error);
                    }
                }
            }
        });

        album_api.hard_delete(Some(tx)).await?;

        let _ = ui_handle.await;

        println!("\n  Finished deleting album contents.");
        Ok(())
    }
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
