# 🛠 Pixeldrain CLI

A simple command-line client for uploading files and directories to [Pixeldrain](https://pixeldrain.com).

-> Join [Matrix Chat Room](https://matrix.to/#/#pixeldrain_cli:matrix.org)
<br>
# Pixeldrain-cli files upload demo.
![pixeldrain-cli upload](https://github.com/ForTheCommunity/Assets/blob/main/pixeldrain-cli/pixeldrain-cli_upload.gif?raw=true)
<br>

## ❖ Features

- **Upload individual files** to Pixeldrain
- **Batch upload multiple files** in a single command
- **Recursive directory uploads** — automatically discovers files inside subdirectories
- **Filter uploads by file extension** (e.g. `mp4`, `jpg`, `png`)
- **Natural file ordering** for predictable upload sequences
- **Concurrent uploads** for smaller files
- **Real-time progress tracking** with per-file and overall progress
- **Persistent upload state** — resume interrupted uploads without re-uploading completed files [[using a state file](#-save-progress-of-uploaded-files-so-it-upload-can-be-resumed-later)].
- **File-size validation** — detect modified files and automatically upload them again [[using a state file](#-save-progress-of-uploaded-files-so-it-upload-can-be-resumed-later)].
- **Optional automatic deletion** of local files after successful upload
- **Create Pixeldrain albums/lists** from uploaded files
- **Automatic album updates** — add newly uploaded files to existing albums
- **Automatically save and reuse album IDs** through the upload state [[using a state file](#-save-progress-of-uploaded-files-so-it-upload-can-be-resumed-later)].
- **List albums/lists** in your Pixeldrain account
- **View files contained in an album/list**
- **Delete albums/lists and their contained files**
- **Secure local API-key storage**
- **Password-based API-key encryption**

## ❖ Installation

## ✦ Download binaries from [releases](https://github.com/ForTheCommunity/Pixeldrain-Cli/releases) Page.

> Binaries are available for Windows OS and Mac OS but are not tested on these platforms.
> App is only tested on Linux Platform.

## ✦ Install using [AppMan](https://github.com/ivan-hc/AppMan) / [AM](https://github.com/ivan-hc/AM).
```
appman install pixeldrain-cli
```

## ❖ Usage

Run:

```bash
pixeldrain-cli --help
```

# ❖ Login

Before uploading files, configure your Pixeldrain API key:

```bash
pixeldrain-cli login
```

The CLI will ask for your API key and a password.

The API key is encrypted before being stored locally.

# ❖ Upload

```text
pixeldrain-cli upload [OPTIONS]
```

| Option | Long form | Description |
|--------|-----------|-------------|
| `-p` | `--paths` | File or directory paths to upload |
| `-a` | `--album` | Create an album with the uploaded files |
| `-i` | `--album-id` | Add uploaded files to an already existing album/list |
| `-f` | `--formats` | File extensions to upload |
| `-d` | `--delete` | delete uploaded files from local storage device [HDD / SSD] |
| `-s` | `--state` | Path to a state file for tracking uploaded files and resuming uploads |


## ✦ Upload a file

```bash
pixeldrain-cli upload -p video.mp4
```

## ✦ Upload multiple files

```bash
pixeldrain-cli upload -p video.mp4 movie.mkv image.jpg
```

## ✦ Upload a directory

```bash
pixeldrain-cli upload -p ./videos
```

Directories are searched recursively.

## ✦ Upload files and directories together

```bash
pixeldrain-cli upload -p video.mp4  ~/Downloads/Videos/ -p ~/Downloads/Photos/
```

## ✦ Filter by file format

Use `-f` / `--formats` to upload only specific file types:

```bash
pixeldrain-cli upload -p ./videos -f mp4 mkv
```

You can also use extensions with a leading dot:

```bash
pixeldrain-cli upload -p ./videos -f .mp4 .mkv
```

## ✦ Delete files after upload

Use `-d` / `--delete` to automatically delete local files after they have been successfully uploaded to Pixeldrain:

```bash
pixeldrain-cli upload -p ./videos -f mp4 mkv -d
```


## ✦ Create an album

Use `-a` / `--album`:

```bash
pixeldrain-cli upload -p ./videos -a "My Videos"
```

After the files are uploaded, the CLI creates a Pixeldrain album containing the uploaded files.

## ✦ Move Uploaded files to already existing album/list

Use `-i` / `--album-id`:

```bash
pixeldrain-cli upload -p ./videos/ -i <Album ID>
```

After the files are uploaded, the CLI adds those files into specified albumn.

## ✦ Save progress of uploaded files, so it upload can be resumed later.

Use `-s` / `--state`:

```bash
# Creating state for first time
pixeldrain-cli upload -p ./videos/ --state <path-to-state-file>
# or using short flag:
pixeldrain-cli upload -p ./videos/ -a "My Videos" -s ./videos_upload_state
# state file is a JSON file but not necessary to give file extension for state file.
```

```bash
# Resume uploads:
pixeldrain-cli upload -p ./videos/ -a "My Videos" -s ./videos_upload_state
# Once pixeldrain album is created
pixeldrain-cli upload -p ./videos/ -s ./videos_upload_state
# albumn id is also stored in state file once album is created in pixeldrain.
```
After the files are uploaded, the CLI adds those files into specified albumn.

## ❖ Album

```text
pixeldrain-cli album <SUBCOMMAND>
```

Manage albums/lists in your account.

| Subcommand | Alias | Description |
|------------|-------|-------------|
| `list` | `l` | List all albums/lists in your account |
| `files <id>` | `f` | List all files inside an album/list |
| `delete <id>` | `d` | Delete an album only |
| `hard-delete <id>` | `hd` | Hard Delete an album/list and all files inside it |

## ✦ List albums

List all albums/lists in your account:

```bash
pixeldrain-cli album list
# or using alias:
pixeldrain-cli album l
```

## ✦ List files in an album

List all files inside an album/list:

```bash
pixeldrain-cli album files <album_id>
# or using alias:
pixeldrain-cli album f <album_id>
```

## ✦ Delete an album

Delete an album only:

```bash
pixeldrain-cli album delete <album_id>
# or using alias:
pixeldrain-cli album d <album_id>
```

## ✦ Delete an album & all files inside it.

This action deletes album and files of that album permanently !!! :

```bash
pixeldrain-cli album hard-delete <album_id>
# or using alias:
pixeldrain-cli album hd <album_id>
```

Examples:

```bash
# List all albums
pixeldrain-cli album l

# List files in album
pixeldrain-cli album f <album_id>

# Delete an album
pixeldrain-cli album d <album_id>

# Delete an album and its files
pixeldrain-cli album hd <album_id>
```

## ❖ About

```text
pixeldrain-cli about
```

Display information about the project.

## ❖ API Key Security

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

## ❖ Storage

The encrypted credentials are stored in the platform's standard application-data directory.

The exact location depends on the operating system.

## ✦ Linux

Typically:

```text
~/.local/share/pixeldrain-cli/
```

## ✦ macOS

Typically:

```text
~/Library/Application Support/pixeldrain-cli/
```

## ✦ Windows

Typically:

```text
%LOCALAPPDATA%\pixeldrain-cli\
```

The CLI uses the platform's appropriate application-data directory rather than placing credentials inside the project directory.




## ❖ Todos :
- [ ] Improve code quality.
- [ ] Configurable upload concurrency.


## ❖ Support
If you find this project useful, consider supporting its development.
<br>
**Monero (XMR):**
`83eg4LiD5PEWGu6JpU2mfQVmVdNJQfKzGAi5GUGZKBkBdWBaGxxUrifCj1WyiUEtUfLNaxQjcfHDaDtxfZhr7RboPCVvTYf`


## ❖ License

This project is licensed under the **[Unlicense](https://unlicense.org)**. You can view the full license text in the [UNLICENSE](./UNLICENSE) file.



## ❖ Disclaimer

This project is an unofficial command-line client for Pixeldrain.

Pixeldrain is a trademark/service of [Fornax](https://fornaxian.tech).


## Contributors :
1. ✦ [ ⧼ Shaswot Nepal ⧽ ](https://ShaswotDhungana.com.np) &nbsp;&nbsp;&nbsp; ｢ Maintainer ｣
