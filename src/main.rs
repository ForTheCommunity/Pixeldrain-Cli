use clap::{CommandFactory, Parser};
use pixeldrain_cli::{
    app::about::show_about,
    cli::{
        AlbumCli, Cli,
        Commands::{self},
        commands::{AlbumHandler, create_ks, download, upload},
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

        Some(Commands::Download(args)) => match download(args).await {
            Ok(_) => {}
            Err(e) => println!("Error -> {}", e),
        },

        Some(Commands::Album { album_cli }) => match album_cli {
            AlbumCli::List => AlbumHandler::list_all().await,
            AlbumCli::Files { id } => AlbumHandler::all_files(id).await,
            AlbumCli::Delete { id } => AlbumHandler::delete(id).await,
            AlbumCli::HardDelete { id } => AlbumHandler::hard_delete(id).await,
        },

        Some(Commands::CreateKeystore { file_path }) => {
            if let Err(e) = create_ks(&file_path) {
                eprintln!("   Error -> {}", e.to_string())
            }
        }

        Some(Commands::About) => show_about(),

        None => Cli::command().print_help().expect("failed to print help"),
    }
}
