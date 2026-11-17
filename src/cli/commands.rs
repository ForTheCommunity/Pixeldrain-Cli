use anyhow::Result;
use transformer::crypt::keystore::create_keystore;

use crate::{
    api::client::ApiClient,
    app::{
        album::AlbumAction,
        downloader::Downloader,
        uploader::{UploadPipeline, UploadPipelineOptions},
    },
    cli::{DownloadArgs, UploadArgs},
    core::{
        password::resolve_secret_confirmed,
        storage::{AuthOptions, resolve_api_key},
    },
};

pub async fn upload(args: UploadArgs, auth: &AuthOptions) -> Result<()> {
    let up_opts = UploadPipelineOptions {
        paths: args.paths,
        album_name: args.album,
        album_id: args.album_id,
        delete_after: args.delete,
        formats: args.formats,
        state: args.state,
        enc_password: args.encrypt_pass,
        enc_keystore: args.encrypt_keystore,
        enc_keystore_pass: args.keystore_pass,
        chunk_size: args.chunk_size,
        ..Default::default()
    };

    let api_key = resolve_api_key(auth)?;

    let upload_pipeline = UploadPipeline::new(ApiClient::default().with_api_key(api_key), up_opts);
    upload_pipeline.run().await?;
    Ok(())
}

pub async fn download(args: DownloadArgs, auth: &AuthOptions) -> Result<()> {
    let api_key = resolve_api_key(auth)?;
    let downloader = Downloader {
        urls: args.links,
        output_path: args.path,
        keystore_path: args.decrypt_keystore,
        keystore_pass: args.keystore_pass,
        password: args.decrypt_pass,
        state: args.state,
        chunk_size: args.chunk_size,
    };
    Downloader::start(downloader, ApiClient::default().with_api_key(api_key)).await?;

    Ok(())
}

pub struct AlbumHandler {}

impl AlbumHandler {
    pub async fn list_all(auth: &AuthOptions) -> Result<()> {
        AlbumAction::show_all_with_auth(auth).await
    }

    pub async fn all_files(id: String, auth: &AuthOptions) -> Result<()> {
        AlbumAction::list_album_with_auth(id, auth).await
    }

    pub async fn delete(id: String, auth: &AuthOptions) -> Result<()> {
        AlbumAction::delete_with_auth(id, auth).await
    }

    pub async fn hard_delete(id: String, auth: &AuthOptions) -> Result<()> {
        AlbumAction::hard_delete_with_auth(id, auth).await
    }
}

pub fn create_ks(path: &std::path::Path, keystore_pass: Option<&str>) -> anyhow::Result<()> {
    let keystore_path = if path.is_dir() {
        path.join("keystore.json")
    } else {
        path.to_path_buf()
    };

    let password = resolve_secret_confirmed(
        keystore_pass,
        "PIXELDRAIN_KEYSTORE_PASS",
        "    Enter New Master password: ",
        "    Confirm Password: ",
        " Password didn't match !!!",
    )?;

    create_keystore(&keystore_path, &password).map_err(|e| anyhow::anyhow!("{}", e))?;
    println!(" Generated Keystore at {}", keystore_path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use transformer::crypt::check_master_password;

    #[test]
    fn test_create_ks_non_interactive() {
        let temp_dir = std::env::temp_dir().join(format!(
            "pixeldrain_cli_ks_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        let ks_path = temp_dir.join("test_ks.json");

        create_ks(&ks_path, Some("super_secret_master")).unwrap();
        assert!(ks_path.exists());
        assert!(check_master_password(&ks_path, "super_secret_master").unwrap());
        assert!(!check_master_password(&ks_path, "wrong_pass").unwrap());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
