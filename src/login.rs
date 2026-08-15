use anyhow::{Result, anyhow};
use rpassword::prompt_password;

use crate::{crypto, storage};

pub fn login() -> Result<()> {
    println!(
        "Add your API KEY, API KEY will be saved in your local machine and will be encrypted
     so that other programs can't read it and only you can unlock it. \n
    this password will be needed to decrypt API Key while uploading files so use a rememberable password.
     "
    );
    let api_key = prompt_password("Pixeldrain API key: ")?;

    let password = prompt_password("Encryption password: ")?;
    let confirmation = prompt_password("Confirm password: ")?;

    if password != confirmation {
        return Err(anyhow!("Password didn't matched..."))?;
    }

    let encrypted = crypto::encrypt(&api_key, &password)?;

    storage::save_credentials(&encrypted)?;

    println!("✓ API key encrypted and saved.");

    Ok(())
}
