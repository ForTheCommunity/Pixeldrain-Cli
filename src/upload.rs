use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use reqwest::Client;

use crate::auth;

fn collect_files(paths: &[PathBuf], formats: Option<&[String]>) -> Result<Vec<PathBuf>> {
    // ALL FILES
    let mut files: Vec<PathBuf> = Vec::new();

    for path in paths {
        if !path.exists() {
            bail!("Path Doesn't Exists !!! {}", path.display());
        }

        if path.is_file() {
            if matches_format(path, formats) {
                files.push(path.clone());
            }
        } else if path.is_dir() {
            collect_files_from_dir(path, formats, &mut files)?;
        } else {
            bail!("Path is neither a file nor a directory: {}", path.display());
        }
    }

    files.sort();
    // removing duplicate entries
    // eg. ./videos/1.mp4 & ./videos/
    // there will be two 1.mp4 so removing duplicate entries...
    files.dedup();

    Ok(files)
}

fn collect_files_from_dir(
    directory: &Path,
    formats: Option<&[String]>,
    files: &mut Vec<PathBuf>,
) -> Result<()> {
    let dir_entries = std::fs::read_dir(directory)
        .with_context(|| format!("Failed to read directory: {} ", directory.display()))?;

    for entry in dir_entries {
        let dir_entry = entry?;
        let path = dir_entry.path();

        if path.is_file() {
            if matches_format(&path, formats) {
                files.push(path);
            }
        } else if path.is_dir() {
            collect_files_from_dir(&path, formats, files)?;
        }
    }

    Ok(())
}

fn matches_format(path: &Path, formats: Option<&[String]>) -> bool {
    let Some(formats) = formats else {
        return true;
    };

    let Some(extension) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };

    let extension = extension.to_ascii_lowercase();

    formats.iter().any(|format| {
        format
            .trim_start_matches('.')
            .eq_ignore_ascii_case(&extension)
    })
}

#[allow(dead_code)]
pub async fn upload(
    paths: &Vec<PathBuf>,
    album: &Option<String>,
    formats: Option<&[String]>,
) -> Result<()> {
    let api_key = auth::get_api_key()?;

    let files = collect_files(paths, formats)?;

    let total_files = files.len();

    println!("Total Files : {}", total_files);

    println!("Files to Upload :");

    for file in &files {
        if let Some(name) = file.file_name().and_then(|n| n.to_str()) {
            println!(" {} ", name);
        }
    }

    // uploading
    let client = reqwest::Client::new();
    let mut file_ids: Vec<String> = Vec::new();
    for a_file in files {
        let filename = a_file
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file");

        let part = reqwest::multipart::Part::file(&a_file)
            .await?
            .file_name(filename.to_string());

        let form = reqwest::multipart::Form::new().part("file", part);

        let response = client
            .post("https://pixeldrain.com/api/file")
            .basic_auth("", Some(&api_key))
            .multipart(form)
            .send()
            .await?;

        let status = response.status();
        let body = response.text().await?;

        if status.is_success() {
            let response_json: serde_json::Value = serde_json::from_str(&body)?;
            if let Some(file_id) = response_json["id"].as_str() {
                file_ids.push(file_id.to_string());
            }
            println!("✓ {} uploaded", filename);
            println!("  Response: {}", body);
        } else {
            eprintln!("✗ {} failed: HTTP {}", filename, status);
            eprintln!("  Response: {}", body);
        }
    }

    // moving to albumn...
    if let Some(album_name) = album {
        if !file_ids.is_empty() {
            create_album(&client, &api_key, album_name, &file_ids).await?;
        }
    }

    Ok(())
}

async fn create_album(
    http_client: &Client,
    api_key: &str,
    album_name: &str,
    file_ids: &[String],
) -> Result<()> {
    println!("");

    let files = file_ids
        .iter()
        .map(|id| {
            serde_json::json!({
                "id": id
            })
        })
        .collect::<Vec<_>>();

    let payload = serde_json::json!({
        "title": album_name,
        "files": files,
    });

    println!(
        "\nCreating album '{}' with {} file(s)...",
        album_name,
        file_ids.len()
    );

    let response = http_client
        .post("https://pixeldrain.com/api/list")
        .basic_auth("", Some(api_key))
        .json(&payload)
        .send()
        .await?;

    let status = response.status();
    let body = response.text().await?;

    if !status.is_success() {
        anyhow::bail!("Failed to create album: HTTP {}\n{}", status, body);
    }

    let response_json: serde_json::Value = serde_json::from_str(&body)?;

    if let Some(album_id) = response_json["id"].as_str() {
        println!("✓ Album created successfully!");
        println!("  https://pixeldrain.com/l/{}", album_id);
    } else {
        anyhow::bail!("Album response did not contain an ID");
    }

    Ok(())
}
