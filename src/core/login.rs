use anyhow::{Result, anyhow};

use crate::core::{
    crypto,
    password::{env_non_empty, resolve_secret, resolve_secret_confirmed},
    storage::{self, AuthOptions},
};

pub fn login() -> Result<()> {
    login_with_auth(&AuthOptions::default())
}

/// Logs out by deleting locally stored credentials.
pub fn logout() -> Result<bool> {
    storage::delete_credentials()
}

/// Logs in and saves encrypted credentials, using CLI/env overrides when provided
/// and prompting interactively only for missing values.
pub fn login_with_auth(auth: &AuthOptions) -> Result<()> {
//     println!(
// 
//         
//         "Add your API KEY, API KEY will be saved in your local machine and will be encrypted \n  
//         so that other programs can't read it and only you can unlock it. \n
//         this password will be needed to decrypt API Key while uploading files 
//         so use a rememberable password."
//     );

    let needs_api_prompt =
        auth.api_key.is_none() && env_non_empty("PIXELDRAIN_API_KEY").is_none();

    if needs_api_prompt {
        println!(
            r#"
    🛠 API KEY SETUP

    Add your API KEY.

    Your API KEY will be saved on your local machine and encrypted
    so that other programs cannot read it and only you can unlock it.

    This password will be needed to decrypt your API KEY while
    uploading files, so please use a memorable password.
"#
        );
    }

    let api_key = resolve_secret(
        auth.api_key.as_deref(),
        "PIXELDRAIN_API_KEY",
        "Pixeldrain API key: ",
    )?;

    if api_key.trim().is_empty() {
        return Err(anyhow!("API key cannot be empty."));
    }

    let password = resolve_secret_confirmed(
        auth.password.as_deref(),
        "PIXELDRAIN_PASSWORD",
        "Encryption password: ",
        "Confirm password: ",
        "Password didn't matched...",
    )?;

    let encrypted = crypto::encrypt(&api_key, &password)?;

    storage::save_credentials(&encrypted)?;

    println!("✓ API key encrypted and saved.");

    Ok(())
}
