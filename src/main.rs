use clap::{CommandFactory, Parser};
use pixeldrain_cli::{
    app::about::show_about,
    cli::{
        AlbumCli, Cli,
        Commands::{self},
        commands::{AlbumHandler, create_ks, download, upload},
    },
    core::login::login_with_auth,
};

#[tokio::main]
async fn main() {
    let cli_args = Cli::parse();
    let auth = cli_args.auth_options();

    match cli_args.command {
        Some(Commands::Login) => {
            if let Err(e) = login_with_auth(&auth) {
                eprintln!("Error : {}", e);
                std::process::exit(1);
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

            if let Err(e) = upload(args, &auth).await {
                eprintln!("Error -> {}", e);
                std::process::exit(1);
            }
        }

        Some(Commands::Download(args)) => {
            if let Err(e) = download(args, &auth).await {
                eprintln!("Error -> {}", e);
                std::process::exit(1);
            }
        }

        Some(Commands::Album { album_cli }) => {
            let res = match album_cli {
                AlbumCli::List => AlbumHandler::list_all(&auth).await,
                AlbumCli::Files { id } => AlbumHandler::all_files(id, &auth).await,
                AlbumCli::Delete { id } => AlbumHandler::delete(id, &auth).await,
                AlbumCli::HardDelete { id } => AlbumHandler::hard_delete(id, &auth).await,
            };
            if let Err(e) = res {
                eprintln!("  Error : {}", e);
                std::process::exit(1);
            }
        }

        Some(Commands::CreateKeystore {
            file_path,
            keystore_pass,
        }) => {
            let pass = keystore_pass.as_deref().or(auth.password.as_deref());
            if let Err(e) = create_ks(&file_path, pass) {
                eprintln!("   Error -> {}", e);
                std::process::exit(1);
            }
        }

        Some(Commands::About) => show_about(),

        None => Cli::command().print_help().expect("failed to print help"),
    }
}
