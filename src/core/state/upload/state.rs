use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::core::hash::compute_b3hash;

/// Version of the on-disk upload state format.
const STATE_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Debug)]
pub struct UploadState {
    // Version of the state-file format.
    pub version: u32,
    // album id
    #[serde(default)]
    pub album_id: Option<String>,
    // Maps a file name to information about its
    // successfully uploaded Pixeldrain file.
    #[serde(default)]
    pub files: HashMap<String, StateFile>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StateFile {
    // file id of uploaded file, returned by server.
    pub id: String,
    // Size of the local file at the time it was uploaded.
    pub size: u64,
    // blake3 hash of original file.
    #[serde(default)]
    pub blake3_hash: String,
}

impl Default for UploadState {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            album_id: None,
            files: HashMap::new(),
        }
    }
}

impl UploadState {
    // loads state from disk.
    pub async fn load(path: &Path) -> Result<Self> {
        let data = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("Failed to read state file: {}", path.display()))?;

        let state: Self = serde_json::from_str(&data)
            .with_context(|| format!("Invalid state file: {}", path.display()))?;

        // Prevent the program from silently interpreting
        // a future/incompatible state format.
        if state.version != STATE_VERSION {
            bail!(
                "Unsupported state file version {}. Expected version {}.",
                state.version,
                STATE_VERSION
            );
        }

        Ok(state)
    }

    // save state to disk
    pub async fn save(&self, path: &Path) -> Result<()> {
        let data = serde_json::to_string_pretty(self)?;

        let tmp_path = Self::temporary_path(path);

        // We first write to a temporary file and only replace
        // the real state file after the temporary write succeeds.
        // This reduces the chance of ending up with a corrupt
        // state file if the process is interrupted while saving.
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

    /// Returns the saved Pixeldrain file ID if this local file
    /// matches by file name, size, and BLAKE3 hash.
    ///
    /// A changed or legacy (unhashed) file is treated as a cache miss.
    pub async fn file_lookup(
        &self,
        file_name: &str,
        file_size: u64,
        file_path: &Path,
        chunk_size: usize,
    ) -> Result<Option<&str>> {
        if let Some(file) = self.files.get(file_name) {
            if file.size == file_size && !file.blake3_hash.is_empty() {
                let file_hash = compute_b3hash(chunk_size, file_size, file_path).await?;
                if file_hash == file.blake3_hash {
                    return Ok(Some(file.id.as_str()));
                }
            }
        }

        Ok(None)
    }

    /// Records a successfully uploaded file.
    pub fn add_file(&mut self, file_name: String, file_id: String, size: u64, blake3_hash: String) {
        self.files.insert(
            file_name,
            StateFile {
                id: file_id,
                size,
                blake3_hash,
            },
        );
    }

    // get album id
    pub fn get_album_id(&self) -> Option<&str> {
        self.album_id.as_deref()
    }

    // set album id
    pub fn set_album_id(&mut self, album_id: String) {
        self.album_id = Some(album_id);
    }

    // Returns the Pixeldrain file IDs of all successfully uploaded files
    // currently recorded in the state.
    pub fn get_all_file_ids(&self) -> Vec<String> {
        self.files.values().map(|file| file.id.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_upload_state_save_load_and_hash_lookup() {
        let base = std::env::temp_dir().join(format!(
            "pixeldrain_cli_state_test_{}",
            std::process::id()
        ));
        let dir_a = base.join("dir_a");
        let dir_b = base.join("dir_b");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&dir_a).unwrap();
        std::fs::create_dir_all(&dir_b).unwrap();

        let file_a = dir_a.join("sample.bin");
        let file_b = dir_b.join("sample.bin");
        let content_1 = b"hello world 1234";
        std::fs::write(&file_a, content_1).unwrap();
        std::fs::write(&file_b, content_1).unwrap();

        let hash_1 = compute_b3hash(1024, content_1.len() as u64, &file_a)
            .await
            .unwrap();

        let mut state = UploadState::default();
        state.set_album_id("album123".to_string());
        state.add_file(
            "sample.bin".to_string(),
            "file123".to_string(),
            content_1.len() as u64,
            hash_1.clone(),
        );

        let state_path = base.join("state.json");
        state.save(&state_path).await.unwrap();

        let loaded = UploadState::load(&state_path).await.unwrap();
        assert_eq!(loaded.get_album_id(), Some("album123"));
        assert!(loaded.files.contains_key("sample.bin"));
        assert_eq!(loaded.files["sample.bin"].blake3_hash, hash_1);

        // Matches from a different directory path because lookup is by filename + size + hash.
        assert_eq!(
            loaded
                .file_lookup("sample.bin", content_1.len() as u64, &file_b, 1024)
                .await
                .unwrap(),
            Some("file123")
        );

        // Same filename and same byte length, but modified content -> cache miss.
        let content_modified_same_len = b"HELLO WORLD 1234";
        assert_eq!(content_modified_same_len.len(), content_1.len());
        std::fs::write(&file_b, content_modified_same_len).unwrap();
        assert_eq!(
            loaded
                .file_lookup("sample.bin", content_1.len() as u64, &file_b, 1024)
                .await
                .unwrap(),
            None
        );

        // Different size -> cache miss.
        assert_eq!(
            loaded
                .file_lookup("sample.bin", (content_1.len() + 1) as u64, &file_b, 1024)
                .await
                .unwrap(),
            None
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn test_upload_state_legacy_entry_without_hash_misses() {
        let base = std::env::temp_dir().join(format!(
            "pixeldrain_cli_state_legacy_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let file = base.join("legacy.txt");
        std::fs::write(&file, b"abc").unwrap();

        let state_path = base.join("legacy_state.json");
        let legacy_json = r#"{
  "version": 1,
  "album_id": "alb_old",
  "files": {
    "legacy.txt": { "id": "old_id", "size": 3 }
  }
}"#;
        std::fs::write(&state_path, legacy_json).unwrap();

        let loaded = UploadState::load(&state_path).await.unwrap();
        assert_eq!(loaded.get_album_id(), Some("alb_old"));
        assert_eq!(
            loaded
                .file_lookup("legacy.txt", 3, &file, 1024)
                .await
                .unwrap(),
            None
        );

        let _ = std::fs::remove_dir_all(&base);
    }
}
