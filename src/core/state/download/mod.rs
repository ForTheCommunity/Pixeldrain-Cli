use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::core::hash::compute_b3hash;

/// Version of the on-disk download state format.
const STATE_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Debug)]
pub struct DownloadState {
    // Version of the state-file format.
    pub version: u32,
    // Maps a Pixeldrain file ID to information about its
    // successfully downloaded (and optionally decrypted) local file.
    #[serde(default)]
    pub files: HashMap<String, StateDownloadFile>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StateDownloadFile {
    // Local file name on disk (restored filename if decrypted, otherwise remote filename).
    pub file_name: String,
    // Size of the local file on disk in bytes.
    pub size: u64,
    // BLAKE3 hash of the local file on disk.
    #[serde(default)]
    pub blake3_hash: String,
    // Whether the recorded local file was produced after decryption.
    #[serde(default)]
    pub decrypted: bool,
}

impl Default for DownloadState {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            files: HashMap::new(),
        }
    }
}

impl DownloadState {
    /// Loads download state from disk.
    pub async fn load(path: &Path) -> Result<Self> {
        let data = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("Failed to read state file: {}", path.display()))?;

        let state: Self = serde_json::from_str(&data)
            .with_context(|| format!("Invalid state file: {}", path.display()))?;

        if state.version != STATE_VERSION {
            bail!(
                "Unsupported state file version {}. Expected version {}.",
                state.version,
                STATE_VERSION
            );
        }

        Ok(state)
    }

    /// Atomically saves download state to disk using a temporary file + rename.
    pub async fn save(&self, path: &Path) -> Result<()> {
        let data = serde_json::to_string_pretty(self)?;

        let tmp_path = Self::temporary_path(path);

        tokio::fs::write(&tmp_path, data)
            .await
            .with_context(|| format!("Failed to write tmp state file {}", tmp_path.display()))?;

        tokio::fs::rename(&tmp_path, path)
            .await
            .with_context(|| format!("Failed to save state file {}", path.display()))?;

        Ok(())
    }

    fn temporary_path(path: &Path) -> PathBuf {
        let mut tmp = path.as_os_str().to_os_string();
        tmp.push(".tmp");
        PathBuf::from(tmp)
    }

    /// Checks whether `file_id` is already downloaded at `output_path` with matching
    /// decryption mode, file size, and BLAKE3 hash.
    pub async fn file_lookup(
        &self,
        file_id: &str,
        output_path: &Path,
        decrypted: bool,
        chunk_size: usize,
    ) -> Result<bool> {
        let Some(saved) = self.files.get(file_id) else {
            return Ok(false);
        };

        if saved.decrypted != decrypted || saved.blake3_hash.is_empty() {
            return Ok(false);
        }

        let local_path = if output_path.is_dir() {
            output_path.join(&saved.file_name)
        } else {
            output_path.to_path_buf()
        };

        let metadata = match tokio::fs::metadata(&local_path).await {
            Ok(meta) if meta.is_file() => meta,
            _ => return Ok(false),
        };

        if metadata.len() != saved.size {
            return Ok(false);
        }

        let file_hash = compute_b3hash(chunk_size, saved.size, &local_path).await?;
        Ok(file_hash == saved.blake3_hash)
    }

    /// Records a successfully downloaded (and optionally decrypted) file.
    pub fn add_file(
        &mut self,
        file_id: String,
        file_name: String,
        size: u64,
        blake3_hash: String,
        decrypted: bool,
    ) {
        self.files.insert(
            file_id,
            StateDownloadFile {
                file_name,
                size,
                blake3_hash,
                decrypted,
            },
        );
    }

    /// Returns all Pixeldrain file IDs currently recorded in the state.
    pub fn get_all_file_ids(&self) -> Vec<String> {
        self.files.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_download_state_save_load_and_lookup() {
        let base = std::env::temp_dir().join(format!(
            "pixeldrain_cli_dl_state_test_{}",
            std::process::id()
        ));
        let out_dir = base.join("downloads");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&out_dir).unwrap();

        let local_file = out_dir.join("movie.mp4");
        let content = b"downloaded payload bytes";
        std::fs::write(&local_file, content).unwrap();

        let hash = compute_b3hash(1024, content.len() as u64, &local_file)
            .await
            .unwrap();

        let mut state = DownloadState::default();
        state.add_file(
            "pd_file_1".to_string(),
            "movie.mp4".to_string(),
            content.len() as u64,
            hash.clone(),
            false,
        );

        let state_path = base.join("dl_state.json");
        state.save(&state_path).await.unwrap();

        let loaded = DownloadState::load(&state_path).await.unwrap();
        assert_eq!(loaded.get_all_file_ids(), vec!["pd_file_1".to_string()]);
        assert_eq!(loaded.files["pd_file_1"].blake3_hash, hash);

        // Exact match -> skip
        assert!(
            loaded
                .file_lookup("pd_file_1", &out_dir, false, 1024)
                .await
                .unwrap()
        );

        // Decryption mode mismatch -> re-download
        assert!(
            !loaded
                .file_lookup("pd_file_1", &out_dir, true, 1024)
                .await
                .unwrap()
        );

        // Same size but corrupted/modified local file -> re-download
        let modified_same_len = b"DOWNLOADED payload bytes";
        assert_eq!(modified_same_len.len(), content.len());
        std::fs::write(&local_file, modified_same_len).unwrap();
        assert!(
            !loaded
                .file_lookup("pd_file_1", &out_dir, false, 1024)
                .await
                .unwrap()
        );

        // Missing local file -> re-download
        std::fs::remove_file(&local_file).unwrap();
        assert!(
            !loaded
                .file_lookup("pd_file_1", &out_dir, false, 1024)
                .await
                .unwrap()
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn test_download_state_single_file_output_path() {
        let base = std::env::temp_dir().join(format!(
            "pixeldrain_cli_dl_state_single_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let custom_out = base.join("custom_name.bin");
        let content = b"single file data";
        std::fs::write(&custom_out, content).unwrap();

        let hash = compute_b3hash(1024, content.len() as u64, &custom_out)
            .await
            .unwrap();

        let mut state = DownloadState::default();
        state.add_file(
            "pd_single".to_string(),
            "custom_name.bin".to_string(),
            content.len() as u64,
            hash,
            true,
        );

        assert!(
            state
                .file_lookup("pd_single", &custom_out, true, 1024)
                .await
                .unwrap()
        );

        let _ = std::fs::remove_dir_all(&base);
    }
}
