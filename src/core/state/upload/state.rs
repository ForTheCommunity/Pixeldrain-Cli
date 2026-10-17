use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// Version of the on-disk upload state format.
const STATE_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Debug)]
pub struct UploadState {
    // Version of the state-file format.
    pub version: u32,
    // album id
    #[serde(default)]
    pub album_id: Option<String>,
    // Maps a local file path to information about its
    // successfully uploaded Pixeldrain file.
    #[serde(default)]
    pub files: HashMap<PathBuf, StateFile>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StateFile {
    // file id of uploaded file, returned by server.
    pub id: String,
    // Size of the local file at the time it was uploaded.
    pub size: u64,
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
    /// is already recorded with the same file size.
    ///
    /// A changed file is treated as a new file.
    pub fn get_file_id(&self, path: &Path, current_size: u64) -> Option<&str> {
        self.files
            .get(path)
            .filter(|saved| saved.size == current_size)
            .map(|saved| saved.id.as_str())
    }

    /// Records a successfully uploaded file.
    pub fn add_file(&mut self, path: PathBuf, file_id: String, size: u64) {
        self.files.insert(path, StateFile { id: file_id, size });
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
