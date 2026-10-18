use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

pub mod commands;

#[derive(Parser)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Login to Pixeldrain.
    Login,
    /// Upload File/s.
    Upload(UploadArgs),

    /// Download File/s.
    Download(DownloadArgs),

    /// Interact with Albums/Lists.
    Album {
        #[command(subcommand)]
        album_cli: AlbumCli,
    },

    /// Create Keystore
    CreateKeystore {
        /// Output path for keystore
        #[arg(short = 'o', long)]
        file_path: PathBuf,
    },

    /// Show information about this project.
    About,
}

#[derive(Args, Debug)]
pub struct DownloadArgs {
    /// link/s of file/s to download.
    #[arg(short = 'l', long, num_args = 1..)]
    pub links: Vec<String>,

    /// output path where file/s will be downloaded & saved.
    #[arg(short = 'p', long)]
    pub path: PathBuf,

    /// Decrypt downloaded files with password.
    #[arg(short = 'd', long, conflicts_with = "decrypt_keystore")]
    pub decrypt_pass: Option<Option<String>>,

    /// Decrypt downloaded files with keystore.
    #[arg(
        short = 'k',
        long,
        conflicts_with = "decrypt_pass",
        value_name = "KEYSTORE_PATH"
    )]
    pub decrypt_keystore: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct UploadArgs {
    /// file/s or Folder/s Path to upload.
    #[arg(short = 'p', long, num_args = 1..)]
    pub paths: Vec<PathBuf>,

    /// Move uploaded files to a new album \ list.
    #[arg(short = 'a', long, num_args = 1..,value_name = "ALBUM NAME")]
    pub album: Option<Vec<String>>,

    /// Add uploaded files to an already existing album/list by its ID.
    #[arg(short = 'i', long, conflicts_with = "album", value_name = "ALBUM ID")]
    album_id: Option<String>,

    /// File Format Filter, only files with specified filter will be upload.
    /// eg : -f mp4 mkv jpeg
    #[arg(short = 'f', long, num_args = 1..)]
    pub formats: Option<Vec<String>>,

    /// Delete local files after they are successfully uploaded.
    #[arg(short = 'd', long)]
    pub delete: bool,

    /// Path to a state file for tracking uploaded files and resuming uploads.
    #[arg(short = 's', long)]
    state: Option<PathBuf>,

    /// Encrypt file(s) before upload directly using a password (self-contained)
    #[arg(
        long,
        conflicts_with = "encrypt_keystore",
        value_name = "KEYSTORE_PATH"
    )]
    pub encrypt_pass: Option<Option<String>>, // None if flag omitted, Some(None) if flag passed without value, Some(Some(pwd)) if value provided

    /// Encrypt file(s) before upload using a keystore file
    #[arg(long, conflicts_with = "encrypt_pass")]
    pub encrypt_keystore: Option<PathBuf>,

    /// Chunk/Buffer size used while hashing & encrypting file. (1-100 MB, default: 1 MB)
    /// Increase for faster processing on larger files.
    #[arg(short = 'c', long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=100))]
    chunk_size: u8,
}

#[derive(Subcommand)]
pub enum AlbumCli {
    /// List all albums/lists in your account. ( alias : l )
    #[command(alias = "l")]
    List,

    /// List all files inside an album/list. ( alias : f )
    #[command(alias = "f")]
    Files {
        /// Album/list ID.
        id: String,
    },

    /// Delete an album/list [ files inside it won't be deleted ] ( alias : d )
    #[command(alias = "d")]
    Delete {
        /// Album/list ID.
        id: String,
    },

    /// Delete an album/list and all files inside it. ( alias : hd )
    #[command(alias = "hd")]
    HardDelete {
        /// Album/list ID.
        id: String,
    },
}
