use serde::Deserialize;

pub mod album;
pub mod upload;

#[derive(Deserialize)]
pub struct FileInfo {
    pub name: String,
}
