use anyhow::{Context, Result};

use crate::{crypto::decrypt, storage::load_credentials};

pub fn get_api_key() -> Result<String> {
    let credentials = match load_credentials() {
        Ok(credentials) => credentials,
        Err(_) => {
            anyhow::bail!(
                "You are not logged in.\n\
                 Run `pixeldrain-cli login` first."
            );
        }
    };

    let password = rpassword::prompt_password("Decrypt API Key [Enter password for decryption]: ")?;

    decrypt(&credentials, &password).context("Failed to decrypt API key")
}
