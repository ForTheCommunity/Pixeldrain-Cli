// computing blake3 hash

use std::path::PathBuf;

use anyhow::Result;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use tokio::{fs::File, io::AsyncReadExt};

pub async fn compute_b3hash(
    chunk_size: usize,
    file_size: u64,
    file_path: &PathBuf,
) -> Result<String> {
    compute_b3hash_inner(chunk_size, file_size, file_path, None).await
}

pub async fn compute_b3hash_with_pb(
    chunk_size: usize,
    file_size: u64,
    file_path: &PathBuf,
    multi_pb: Option<(&MultiProgress, &ProgressBar)>,
) -> Result<String> {
    compute_b3hash_inner(chunk_size, file_size, file_path, multi_pb).await
}

async fn compute_b3hash_inner(
    chunk_size: usize,
    file_size: u64,
    file_path: &PathBuf,
    multi_pb: Option<(&MultiProgress, &ProgressBar)>,
) -> Result<String> {
    let file_name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");

    let (hash_pb, is_managed) = if let Some((mp, overall)) = multi_pb {
        overall.println(format!("  Computing hash for: {}", file_name));
        let pb = mp.insert_before(overall, ProgressBar::new(file_size));
        (pb, true)
    } else {
        println!("  Computing hash for: {}", file_name);
        (ProgressBar::new(file_size), false)
    };

    hash_pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.cyan} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} (Hashing)")
            .unwrap()
            .progress_chars("=>-"),
    );

    let mut hasher = blake3::Hasher::new();
    let mut source_file = File::open(&file_path).await?;
    let mut buffer = vec![0u8; chunk_size];
    let mut hash_reader = tokio::io::BufReader::new(&mut source_file);

    loop {
        let n = hash_reader.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        hash_pb.inc(n as u64);
    }

    if is_managed {
        if let Some((mp, _)) = multi_pb {
            hash_pb.finish_and_clear();
            mp.remove(&hash_pb);
        }
    } else {
        hash_pb.finish_and_clear();
    }

    Ok(hasher.finalize().to_hex().to_string())
}
