# Pixeldrain CLI

A simple command-line client for uploading files and directories to [Pixeldrain](https://pixeldrain.com).

-> Join [Matrix Chat Room](https://matrix.to/#/#pixeldrain_cli:matrix.org)
<br>
# Pixeldrain-cli files upload demo.
![pixeldrain-cli upload](https://github.com/ForTheCommunity/Assets/blob/main/pixeldrain-cli/pixeldrain-cli_upload.gif?raw=true)
<br>

## Features

- Upload individual files
- Upload multiple files at once
- Upload directories recursively
- Filter files by extension
- Upload files in lexicographical order
- Create a Pixeldrain album from uploaded files
- Securely store your Pixeldrain API key locally
- Encrypt the API key using a user-provided password

## Installation

## --> Download | Install :

## # Download binaries from [releases](https://github.com/ForTheCommunity/Pixeldrain-Cli/releases) Page.

> Binaries are available for Windows OS and Mac OS but are not tested on these platforms.
> App is only tested on Linux Platform.

## # Install using [AppMan](https://github.com/ivan-hc/AppMan) / [AM](https://github.com/ivan-hc/AM).
```
appman install pixeldrain-cli
```

## Usage

Run:

```bash
pixeldrain-cli --help
```

## Login

Before uploading files, configure your Pixeldrain API key:

```bash
pixeldrain-cli login
```

The CLI will ask for your API key and a password.

The API key is encrypted before being stored locally.

## Upload a file

```bash
pixeldrain-cli upload -p video.mp4
```

## Upload multiple files

```bash
pixeldrain-cli upload -p video.mp4 movie.mkv image.jpg
```

## Upload a directory

```bash
pixeldrain-cli upload -p ./videos
```

Directories are searched recursively.

## Upload files and directories together

```bash
pixeldrain-cli upload -p video.mp4  ~/Downloads/Videos/ -p ~/Downloads/Photos/
```

## Filter by file format

Use `-f` / `--formats` to upload only specific file types:

```bash
pixeldrain-cli upload -p ./videos -f mp4 mkv
```

You can also use extensions with a leading dot:

```bash
pixeldrain-cli upload -p ./videos -f .mp4 .mkv
```

## Delete files after upload

Use `-d` / `--delete` to automatically delete local files after they have been successfully uploaded to Pixeldrain:

```bash
pixeldrain-cli upload -p ./videos -f mp4 mkv -d
```


## Create an album

Use `-a` / `--album`:

```bash
pixeldrain-cli upload -p ./videos -a "My Videos"
```

After the files are uploaded, the CLI creates a Pixeldrain album containing the uploaded files.

## Command Reference

## `login`

```text
pixeldrain-cli login
```

Configure and securely store your Pixeldrain API key.

## `upload`

```text
pixeldrain-cli upload [OPTIONS]
```

| Option | Long form | Description |
|--------|-----------|-------------|
| `-p` | `--paths` | File or directory paths to upload |
| `-a` | `--album` | Create an album with the uploaded files |
| `-f` | `--formats` | File extensions to upload |
| `-d` | `--delete` | delete uploaded files from local storage device [HDD / SSD] |

Example:

```bash
pixeldrain-cli upload -p ./movies ./series -a "My Collection" -f mp4 mkv
```

## API Key Security

The CLI does not store the API key as plain text.

During login:

```text
API Key
   |
   v
User password
   |
   v
Argon2
   |
   v
AES-256-GCM
   |
   v
Encrypted local storage
```

The encryption password is required to decrypt the API key.

The encryption key is derived from the user's password using Argon2, and the API key is encrypted using AES-256-GCM.

> Keep your encryption password safe. If you lose it, the stored API key cannot be recovered.

## Storage

The encrypted credentials are stored in the platform's standard application-data directory.

The exact location depends on the operating system.

## Linux

Typically:

```text
~/.local/share/pixeldrain-cli/
```

## macOS

Typically:

```text
~/Library/Application Support/pixeldrain-cli/
```

## Windows

Typically:

```text
%LOCALAPPDATA%\pixeldrain-cli\
```

The CLI uses the platform's appropriate application-data directory rather than placing credentials inside the project directory.




## Todos :

- [ ] Resume interrupted uploads | save state/progres of uploaded files and resume uploading remaining files.
- [ ] Parallel uploads
- [ ] Configurable upload concurrency
- [ ] More detailed error handling


## Support
If you find this project useful, consider supporting its development.
<br>
**Monero (XMR):**
`83eg4LiD5PEWGu6JpU2mfQVmVdNJQfKzGAi5GUGZKBkBdWBaGxxUrifCj1WyiUEtUfLNaxQjcfHDaDtxfZhr7RboPCVvTYf`


## License

This project is licensed under the **[Unlicense](https://unlicense.org)**. You can view the full license text in the [UNLICENSE](./UNLICENSE) file.



## Disclaimer

This project is an unofficial command-line client for Pixeldrain.

Pixeldrain is a trademark/service of [Fornax](https://twitter.com/Fornax96).
