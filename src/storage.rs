use std::{fs, path::PathBuf};

use anyhow::{Result, anyhow};
use directories::ProjectDirs;

use crate::crypto::{self, EncryptedData};

pub fn project_data_dir() -> Result<PathBuf> {
    let project_dir = ProjectDirs::from("org", "forthecommunity", "pixeldrain-cli")
        .ok_or_else(|| anyhow!("Error getting project dir !!!"))?;

    let data_dir = project_dir.data_dir();
    fs::create_dir_all(data_dir)?;

    Ok(data_dir.to_path_buf())
}

fn credentials_path() -> Result<PathBuf> {
    Ok(project_data_dir()?.join("credentials.json"))
}
pub fn save_credentials(credentials: &EncryptedData) -> Result<()> {
    let path = credentials_path()?;

    let json = serde_json::to_string_pretty(credentials)?;

    fs::write(path, json)?;

    Ok(())
}

pub fn load_credentials() -> Result<EncryptedData> {
    let path = credentials_path()?;

    let json = fs::read_to_string(path)?;

    let credentials = serde_json::from_str(&json)?;

    Ok(credentials)
}

pub fn get_api_key() -> Result<String> {
    let password = rpassword::prompt_password("Decrypt API KEY (Enter Encryption password): ")?;

    let encrypted = load_credentials()?;

    let api_key = crypto::decrypt(&encrypted, &password)?;

    Ok(api_key)
}
