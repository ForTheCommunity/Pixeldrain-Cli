use serde::{Deserialize, Serialize};
use tabled::Tabled;

#[derive(Serialize, Deserialize, Debug)]
pub struct AlbumListResponse {
    pub lists: Vec<Album>,
}

#[derive(Serialize, Deserialize, Debug, Tabled)]
pub struct Album {
    pub id: String,
    pub title: String,
    pub date_created: String,
    pub file_count: usize,
    pub can_edit: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AlbumDetailResponse {
    pub success: bool,
    pub id: String,
    pub title: String,
    pub date_created: String,
    pub file_count: usize,
    pub can_edit: bool,
    pub files: Vec<AlbumFile>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct AlbumFile {
    pub detail_href: String,

    pub description: String,

    pub id: String,

    pub name: String,

    pub size: u64,

    pub views: u64,

    pub bandwidth_used: u64,

    pub bandwidth_used_paid: u64,

    pub downloads: u64,

    pub date_upload: String,

    pub date_last_view: String,

    pub mime_type: String,

    pub thumbnail_href: String,

    pub hash_sha256: String,

    pub delete_after_date: String,

    pub delete_after_downloads: u64,

    pub availability: String,

    pub availability_message: String,

    pub abuse_type: String,

    pub abuse_reporter_name: String,

    pub can_edit: bool,

    pub can_download: bool,

    pub show_ads: bool,

    pub allow_video_player: bool,

    pub download_speed_limit: u64,

    // Safely capture any future unknown fields without breaking deserialization
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}
