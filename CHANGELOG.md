# Changelog

## [0.1.6] - 2026-11-21
- Added file encryption before upload using password or keystore file via [Transformer](https://gitlab.com/ForTheCommunity/Transformer).
- Added file decryption on download with automatic `.encrypted` file detection and interactive decryption prompts.
- Added `create-keystore` command to create encryption keystores.
- Added `logout` command to remove locally stored credentials from the machine.
- Added `check-update` (alias `update`) command to check for new releases across GitLab & GitHub and view the latest changelog.
- Added raw ID download support with automatic file/album(list) detection.
- Added global `--api-key` and `-P` / `--password` options and environment variable support across all subcommands for seamless scripting.
- Improved CLI error handling to display full subcommand help when required arguments are omitted.
- Added update instructions for Cargo, AM/AppMan, GitLab, and GitHub.
- Expanded release build targets to 12 targets: Linux ARM64 MUSL, ARMv7 (GNU & MUSL), 32-bit x86 i686 (GNU & MUSL), and RISC-V 64.

## [0.1.5] - 2026-10-11
- Decoupled old messy monolithic code

## [0.1.4] - 2026-08-25
- added feature to Move Uploaded files to already existing album/list.
- added feature to resume upload using a state file to track upload.

## [0.1.3] - 2026-08-23
- added feature to manage album/lists.
- improved files uploading.

## [0.1.2] - 2026-08-17
- added delete [ -d ] flag, if enabled it deletes uploaded file from local storage.

## [0.1.1] - 2026-08-16
- added progress bar.
- added Estimated Time for file transfer and overall transfer.
