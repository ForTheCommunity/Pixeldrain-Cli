use std::{fs, path::PathBuf};

use anyhow::{Result, anyhow, bail};
use directories::ProjectDirs;

use crate::core::{
    crypto::{self, EncryptedData},
    password::{env_non_empty, resolve_secret},
};

/// Optional authentication overrides supplied via CLI flags.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthOptions {
    pub api_key: Option<String>,
    pub password: Option<String>,
}

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

    if !path.exists() {
        bail!(
            "You are not logged in.\n\
                Run `pixeldrain-cli login` first."
        );
    }
    let json = fs::read_to_string(path)?;
    let credentials = serde_json::from_str(&json)?;

    Ok(credentials)
}

/// Resolves the Pixeldrain API key from CLI/env overrides, or loads and decrypts
/// stored credentials using the CLI/env password or an interactive prompt.
pub fn resolve_api_key(auth: &AuthOptions) -> Result<String> {
    if let Some(key) = auth.api_key.as_deref() {
        if key.trim().is_empty() {
            bail!("API key cannot be empty.");
        }
        return Ok(key.to_string());
    }

    if let Some(key) = env_non_empty("PIXELDRAIN_API_KEY") {
        if key.trim().is_empty() {
            bail!("API key cannot be empty.");
        }
        return Ok(key);
    }

    let encrypted = load_credentials()?;
    let password = resolve_secret(
        auth.password.as_deref(),
        "PIXELDRAIN_PASSWORD",
        "Decrypt API KEY (Enter Encryption password): ",
    )?;
    let api_key = crypto::decrypt(&encrypted, &password)?;

    Ok(api_key)
}

pub fn get_api_key() -> Result<String> {
    resolve_api_key(&AuthOptions::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_api_key_from_auth_options() {
        let auth = AuthOptions {
            api_key: Some("test-api-key".to_string()),
            password: None,
        };
        assert_eq!(resolve_api_key(&auth).unwrap(), "test-api-key");
    }

    #[test]
    fn test_resolve_api_key_rejects_empty_cli_key() {
        let auth = AuthOptions {
            api_key: Some("   ".to_string()),
            password: None,
        };
        assert!(resolve_api_key(&auth).is_err());
    }
}
