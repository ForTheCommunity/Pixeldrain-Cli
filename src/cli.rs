use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
    Upload {
        /// file/s or Folder/s Path to upload.
        #[arg(short = 'p', long, num_args = 1..)]
        paths: Vec<PathBuf>,
        /// Move uploaded files to a new album \ list.
        #[arg(short = 'a', long)]
        album: Option<String>,
        /// Add uploaded files to an already existing album/list by its ID.
        #[arg(short = 'i', long, conflicts_with = "album")]
        album_id: Option<String>,
        /// File Format Filter, only files with specified filter will be upload.
        /// eg : -f mp4 mkv jpeg
        #[arg(short = 'f', long, num_args = 1..)]
        formats: Option<Vec<String>>,
        /// Delete local files after they are successfully uploaded.
        #[arg(short = 'd', long)]
        delete: bool,
        /// Path to a state file for tracking uploaded files and resuming uploads.
        #[arg(short = 's', long)]
        state: Option<PathBuf>,
    },

    /// Manage Albums/Lists
    Album {
        #[command(subcommand)]
        action: AlbumActions,
    },

    /// Show information about this project.
    About,
}

#[derive(Subcommand)]
pub enum AlbumActions {
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

    /// Hard Delete an album/list and all files inside it. ( alias : hd )
    #[command(alias = "hd")]
    HardDelete {
        /// Album/list ID.
        id: String,
    },
}
