use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use anyhow::{Result, anyhow};
use argon2::Argon2;
use base64::{Engine, engine::general_purpose::STANDARD};

use serde::{Deserialize, Serialize};

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

#[derive(Debug, Serialize, Deserialize)]
pub struct EncryptedData {
    pub version: u8,
    pub salt: String,
    pub nonce: String,
    pub cipher_text: String,
}

#[allow(unused_variables)]
pub fn encrypt(plaintext_api: &str, password: &str) -> anyhow::Result<EncryptedData> {
    let mut salt = [0u8; SALT_LEN];
    rand::fill(&mut salt);

    let mut key = [0u8; KEY_LEN];

    // Derive 256-bit encryption key from password.
    Argon2::default()
        .hash_password_into(password.as_bytes(), &salt, &mut key)
        .map_err(|e| anyhow!(format!("error occurred while hashing password : {}", e)))?;

    // AES-256-GCM cipher.
    let cipher = Aes256Gcm::new_from_slice(&key)?;

    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::fill(&mut nonce_bytes);

    let nonce = Nonce::try_from(nonce_bytes.as_slice())?;

    let cipher_text = cipher.encrypt(&nonce, plaintext_api.as_bytes())?;

    Ok(EncryptedData {
        version: 1,
        salt: STANDARD.encode(salt),
        nonce: STANDARD.encode(nonce),
        cipher_text: STANDARD.encode(cipher_text),
    })
}

pub fn decrypt(data: &EncryptedData, password: &str) -> Result<String> {
    let salt = STANDARD.decode(&data.salt)?;
    let nonce_bytes = STANDARD.decode(&data.nonce)?;
    let cipher_text = STANDARD.decode(&data.cipher_text)?;

    let mut key = [0u8; KEY_LEN];

    Argon2::default()
        .hash_password_into(password.as_bytes(), &salt, &mut key)
        .map_err(|e| anyhow!(format!("error occurred while hashing password : {}", e)))?;

    let cipher = Aes256Gcm::new_from_slice(&key)?;

    let nonce = Nonce::try_from(nonce_bytes.as_slice())?;

    let plaintext = cipher.decrypt(&nonce, cipher_text.as_ref()).map_err(|e| {
        anyhow!(format!(
            "Invalid encryption password or corrupted credentials, {}",
            e
        ))
    })?;

    Ok(String::from_utf8(plaintext)?)
}
