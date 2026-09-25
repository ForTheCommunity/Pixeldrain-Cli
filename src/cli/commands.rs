use anyhow::Result;

use crate::{
    api::client::ApiClient,
    app::{
        album::AlbumAction,
        uploader::{UploadPipeline, UploadPipelineOptions},
    },
    cli::UploadArgs,
    core::storage::get_api_key,
};

pub async fn upload(args: UploadArgs) -> Result<()> {
    let up_opts = UploadPipelineOptions {
        paths: args.paths,
        album_name: args.album,
        delete_after: args.delete,
        formats: args.formats,
        ..Default::default()
    };

    let api_key = get_api_key()?;

    let upload_pipeline = UploadPipeline::new(ApiClient::default().with_api_key(api_key), up_opts);
    upload_pipeline.run().await?;
    Ok(())
}

pub struct AlbumHandler {}

impl AlbumHandler {
    pub async fn list_all() {
        match AlbumAction::show_all().await {
            Ok(_) => {}
            Err(e) => println!("  Error : {}", e),
        }
    }

    pub async fn all_files(id: String) {
        match AlbumAction::list_album(id).await {
            Ok(_) => {}
            Err(e) => println!("  Error : {}", e),
        }
    }

    pub async fn delete(id: String) {
        match AlbumAction::delete(id).await {
            Ok(_) => {}
            Err(e) => println!("  Error : {}", e),
        }
    }

    pub async fn hard_delete(id: String) {
        match AlbumAction::hard_delete(id).await {
            Ok(_) => {}
            Err(e) => println!("  Error : {}", e),
        }
    }
}
