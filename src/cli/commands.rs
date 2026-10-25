use anyhow::Result;
use transformer::crypt::keystore::create_keystore;

use crate::{
    api::client::ApiClient, app::{
        album::AlbumAction,
        downloader::Downloader,
        uploader::{UploadPipeline, UploadPipelineOptions},
    }, cli::{DownloadArgs, UploadArgs}, core::{password::prompt_password, storage::get_api_key},
};

pub async fn upload(args: UploadArgs) -> Result<()> {
    let up_opts = UploadPipelineOptions {
        paths: args.paths,
        album_name: args.album,
        album_id: args.album_id,
        delete_after: args.delete,
        formats: args.formats,
        state: args.state,
        enc_password: args.encrypt_pass,
        enc_keystore: args.encrypt_keystore,
        chunk_size: args.chunk_size,
        ..Default::default()
    };

    let api_key = get_api_key()?;

    let upload_pipeline = UploadPipeline::new(ApiClient::default().with_api_key(api_key), up_opts);
    upload_pipeline.run().await?;
    Ok(())
}

pub async  fn download(args: DownloadArgs) -> Result<()> {
    let api_key = get_api_key()?;
    let downloader = Downloader {
        urls: args.links,
        output_path: args.path,
        keystore_path: args.decrypt_keystore,
        password: args.decrypt_pass
    };
    Downloader::start(downloader, ApiClient::default().with_api_key(api_key)).await?;

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



pub fn create_ks(path: &std::path::Path) -> anyhow::Result<()> {
    let keystore_path = if path.is_dir() {
        path.join("keystore.json")
    } else {
        path.to_path_buf()
    };

    let password = prompt_password("    Enter New Master password: ")?;
    let confirm_password = prompt_password("    Confirm Password: ")?;

    if password != confirm_password {
        return Err(anyhow::anyhow!(" Password didn't match !!!"));
    }

    create_keystore(&keystore_path, &password).map_err(|e| anyhow::anyhow!("{}", e))?;
    println!(" Generated Keystore at {}", keystore_path.display());
    Ok(())
}
