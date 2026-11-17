use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

use crate::core::storage::AuthOptions;

pub mod commands;

#[derive(Parser)]
#[command(version)]
pub struct Cli {
    /// Password to decrypt your stored Pixeldrain API key (or encrypt it during login).
    /// Can also be set via PIXELDRAIN_PASSWORD.
    #[arg(short = 'P', long, global = true, value_name = "PASSWORD")]
    pub password: Option<String>,

    /// Pixeldrain API key (bypasses stored credentials, or sets API key during login).
    /// Can also be set via PIXELDRAIN_API_KEY.
    #[arg(long, global = true, value_name = "API_KEY")]
    pub api_key: Option<String>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

impl Cli {
    pub fn auth_options(&self) -> AuthOptions {
        AuthOptions {
            api_key: self.api_key.clone(),
            password: self.password.clone(),
        }
    }
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

        /// Master password for the new keystore.
        /// Can also be set via PIXELDRAIN_KEYSTORE_PASS.
        #[arg(long, value_name = "PASSWORD")]
        keystore_pass: Option<String>,
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
    /// Prompts if flag is passed without a value (or uses PIXELDRAIN_DECRYPT_PASS).
    #[arg(
        short = 'd',
        long,
        conflicts_with = "decrypt_keystore",
        value_name = "PASSWORD"
    )]
    pub decrypt_pass: Option<Option<String>>,

    /// Decrypt downloaded files with keystore.
    #[arg(
        short = 'k',
        long,
        conflicts_with = "decrypt_pass",
        value_name = "KEYSTORE_PATH"
    )]
    pub decrypt_keystore: Option<PathBuf>,

    /// Master password for the decryption keystore file.
    /// Can also be set via PIXELDRAIN_KEYSTORE_PASS.
    #[arg(
        long,
        requires = "decrypt_keystore",
        conflicts_with = "decrypt_pass",
        value_name = "PASSWORD"
    )]
    pub keystore_pass: Option<String>,

    /// Path to a state file for tracking downloaded files and resuming downloads.
    #[arg(short = 's', long)]
    pub state: Option<PathBuf>,

    /// Chunk/Buffer size used while hashing files for state verification. (1-100 MB, default: 1 MB)
    #[arg(short = 'c', long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=100))]
    pub chunk_size: u8,
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

    /// Encrypt file(s) before upload directly using a password (self-contained).
    /// Prompts if flag is passed without a value (or uses PIXELDRAIN_ENCRYPT_PASS).
    #[arg(
        long,
        conflicts_with = "encrypt_keystore",
        value_name = "PASSWORD"
    )]
    pub encrypt_pass: Option<Option<String>>, // None if flag omitted, Some(None) if flag passed without value, Some(Some(pwd)) if value provided

    /// Encrypt file(s) before upload using a keystore file
    #[arg(long, conflicts_with = "encrypt_pass", value_name = "KEYSTORE_PATH")]
    pub encrypt_keystore: Option<PathBuf>,

    /// Master password for the encryption keystore file.
    /// Can also be set via PIXELDRAIN_KEYSTORE_PASS.
    #[arg(
        long,
        requires = "encrypt_keystore",
        conflicts_with = "encrypt_pass",
        value_name = "PASSWORD"
    )]
    pub keystore_pass: Option<String>,

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_global_auth_flags_before_and_after_subcommand() {
        let cli = Cli::try_parse_from([
            "pixeldrain-cli",
            "-P",
            "secret",
            "--api-key",
            "key123",
            "login",
        ])
        .unwrap();
        assert_eq!(cli.password.as_deref(), Some("secret"));
        assert_eq!(cli.api_key.as_deref(), Some("key123"));
        assert!(matches!(cli.command, Some(Commands::Login)));

        let cli_after = Cli::try_parse_from([
            "pixeldrain-cli",
            "album",
            "list",
            "--password",
            "secret2",
            "--api-key",
            "key456",
        ])
        .unwrap();
        assert_eq!(cli_after.password.as_deref(), Some("secret2"));
        assert_eq!(cli_after.api_key.as_deref(), Some("key456"));
    }

    #[test]
    fn test_upload_encryption_flags() {
        let cli_pass = Cli::try_parse_from([
            "pixeldrain-cli",
            "upload",
            "-p",
            "file.txt",
            "--encrypt-pass",
            "enc123",
            "-P",
            "apipass",
        ])
        .unwrap();
        assert_eq!(cli_pass.password.as_deref(), Some("apipass"));
        match cli_pass.command {
            Some(Commands::Upload(args)) => {
                assert_eq!(args.encrypt_pass, Some(Some("enc123".to_string())));
                assert_eq!(args.paths, vec![PathBuf::from("file.txt")]);
            }
            _ => panic!("expected Upload command"),
        }

        let cli_no_val = Cli::try_parse_from([
            "pixeldrain-cli",
            "upload",
            "--encrypt-pass",
            "-p",
            "file.txt",
        ])
        .unwrap();
        match cli_no_val.command {
            Some(Commands::Upload(args)) => {
                assert_eq!(args.encrypt_pass, Some(None));
                assert_eq!(args.paths, vec![PathBuf::from("file.txt")]);
            }
            _ => panic!("expected Upload command"),
        }

        let cli_ks = Cli::try_parse_from([
            "pixeldrain-cli",
            "upload",
            "-p",
            "file.txt",
            "--encrypt-keystore",
            "ks.json",
            "--keystore-pass",
            "master",
        ])
        .unwrap();
        match cli_ks.command {
            Some(Commands::Upload(args)) => {
                assert_eq!(args.encrypt_keystore, Some(PathBuf::from("ks.json")));
                assert_eq!(args.keystore_pass.as_deref(), Some("master"));
            }
            _ => panic!("expected Upload command"),
        }

        // --keystore-pass requires --encrypt-keystore
        assert!(
            Cli::try_parse_from([
                "pixeldrain-cli",
                "upload",
                "-p",
                "file.txt",
                "--keystore-pass",
                "master",
            ])
            .is_err()
        );
    }

    #[test]
    fn test_download_decryption_flags() {
        let cli_ks = Cli::try_parse_from([
            "pixeldrain-cli",
            "download",
            "-l",
            "https://pixeldrain.com/u/abc",
            "-p",
            "./out",
            "-k",
            "ks.json",
            "--keystore-pass",
            "master",
            "-s",
            "dl_state.json",
            "-c",
            "4",
            "--api-key",
            "key123",
        ])
        .unwrap();
        assert_eq!(cli_ks.api_key.as_deref(), Some("key123"));
        match cli_ks.command {
            Some(Commands::Download(args)) => {
                assert_eq!(args.decrypt_keystore, Some(PathBuf::from("ks.json")));
                assert_eq!(args.keystore_pass.as_deref(), Some("master"));
                assert_eq!(args.state, Some(PathBuf::from("dl_state.json")));
                assert_eq!(args.chunk_size, 4);
            }
            _ => panic!("expected Download command"),
        }

        let cli_pass = Cli::try_parse_from([
            "pixeldrain-cli",
            "download",
            "-l",
            "https://pixeldrain.com/u/abc",
            "-p",
            "./out",
            "-d",
            "dec123",
        ])
        .unwrap();
        match cli_pass.command {
            Some(Commands::Download(args)) => {
                assert_eq!(args.decrypt_pass, Some(Some("dec123".to_string())));
            }
            _ => panic!("expected Download command"),
        }

        // --keystore-pass requires --decrypt-keystore
        assert!(
            Cli::try_parse_from([
                "pixeldrain-cli",
                "download",
                "-l",
                "https://pixeldrain.com/u/abc",
                "-p",
                "./out",
                "--keystore-pass",
                "master",
            ])
            .is_err()
        );
    }

    #[test]
    fn test_create_keystore_flags() {
        let cli = Cli::try_parse_from([
            "pixeldrain-cli",
            "create-keystore",
            "-o",
            "ks.json",
            "--keystore-pass",
            "master",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::CreateKeystore {
                file_path,
                keystore_pass,
            }) => {
                assert_eq!(file_path, PathBuf::from("ks.json"));
                assert_eq!(keystore_pass.as_deref(), Some("master"));
            }
            _ => panic!("expected CreateKeystore command"),
        }
    }
}
