use clap::{CommandFactory, Parser};
use pixeldrain_cli::{
    cli::{
        AlbumCli, Cli, Commands,
        commands::{AlbumHandler, upload},
    },
    core::login::login,
};

#[tokio::main]
async fn main() {
    let cli_args = Cli::parse();

    match cli_args.command {
        Some(Commands::Login) => {
            if let Err(e) = login() {
                println!("Error : {}", e);
            }
        }

        Some(Commands::Upload(args)) => {
            if args.paths.is_empty() {
                let mut cmd = Cli::command();

                if let Some(sub) = cmd.find_subcommand_mut("upload") {
                    let _ = sub.print_help();
                }
                return;
            }

            match upload(args).await {
                Ok(_) => {}
                Err(e) => println!("Error -> {}", e),
            }
        }

        Some(Commands::Album { album_cli }) => match album_cli {
            AlbumCli::List => AlbumHandler::list_all().await,
            AlbumCli::Files { id } => AlbumHandler::all_files(id).await,
            AlbumCli::Delete { id } => AlbumHandler::delete(id).await,
            AlbumCli::HardDelete { id } => AlbumHandler::hard_delete(id).await,
        },

        None => Cli::command().print_help().expect("failed to print help"),
    }
}
