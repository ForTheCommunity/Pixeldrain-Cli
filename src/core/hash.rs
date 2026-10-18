// computing blake3 hash

use std::path::PathBuf;

use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use tokio::{fs::File, io::AsyncReadExt};

pub async fn compute_b3hash(
    chunk_size: usize,
    file_size: u64,
    file_path: &PathBuf,
) -> Result<String> {
    println!(
        "  Computing hash for: {}",
        file_path.file_name().unwrap().to_str().unwrap()
    );
    let hash_pb = ProgressBar::new(file_size);
    hash_pb.set_style(ProgressStyle::default_bar()
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
    hash_pb.finish_with_message("Hashed successfully!");
    Ok(hasher.finalize().to_hex().to_string())
}
