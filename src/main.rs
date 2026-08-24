use clap::{CommandFactory, Parser};
use pixeldrain_cli::album::AlbumAction;
use pixeldrain_cli::cli::{AlbumActions, Cli, Commands};
use pixeldrain_cli::login::login;
use pixeldrain_cli::upload::upload;

#[tokio::main]
async fn main() {
    let cli_args = Cli::parse();

    match &cli_args.command {
        Some(Commands::Login) => {
            match login() {
                Ok(_a) => {}
                Err(e) => println!("Error : {}", e),
            };
        }

        Some(Commands::Upload {
            paths,
            album,
            formats,
            delete,
        }) => {
            if paths.is_empty() {
                let mut cmd = Cli::command();

                cmd.find_subcommand_mut("upload")
                    .unwrap()
                    .print_help()
                    .unwrap();

                return;
            }

            match upload(paths, album.as_deref(), formats.as_deref(), *delete).await {
                Ok(_a) => {}
                Err(e) => println!("Error -> {}", e),
            }
        }

        Some(Commands::Album { action }) => match action {
            AlbumActions::List => match AlbumAction::list_all().await {
                Ok(_) => {}
                Err(e) => {
                    println!("Error -> {}", e)
                }
            },

            AlbumActions::Files { id } => match AlbumAction::all_files(&id).await {
                Ok(_) => {}
                Err(e) => {
                    println!("Error -> {}", e)
                }
            },
            AlbumActions::Delete { id } => match AlbumAction::delete(&id).await {
                Ok(_) => {}
                Err(e) => {
                    println!("Error -> {}", e)
                }
            },
            AlbumActions::HardDelete { id } => match AlbumAction::hard_delete(&id).await {
                Ok(_) => {}
                Err(e) => {
                    println!("Error -> {}", e)
                }
            },
        },

        Some(Commands::About) => {
            pixeldrain_cli::about::print_about();
        }

        None => Cli::command().print_help().expect("failed to print help"),
    }
}
