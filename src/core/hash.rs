// computing blake3 hash

use std::{
    path::Path,
    pin::Pin,
    task::{Context, Poll},
};

use anyhow::Result;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncWrite},
};

pub async fn compute_b3hash(
    chunk_size: usize,
    file_size: u64,
    file_path: &Path,
) -> Result<String> {
    compute_b3hash_inner(chunk_size, file_size, file_path, None).await
}

pub async fn compute_b3hash_with_pb(
    chunk_size: usize,
    file_size: u64,
    file_path: &Path,
    multi_pb: Option<(&MultiProgress, &ProgressBar)>,
) -> Result<String> {
    compute_b3hash_inner(chunk_size, file_size, file_path, multi_pb).await
}

async fn compute_b3hash_inner(
    chunk_size: usize,
    file_size: u64,
    file_path: &Path,
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
    let mut source_file = File::open(file_path).await?;
    let mut buffer = vec![0u8; chunk_size.max(1)];
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

/// An `AsyncWrite` wrapper that optionally computes a BLAKE3 hash on the fly
/// for all bytes successfully written to the inner writer.
pub struct Blake3Writer<W> {
    inner: W,
    blake3: Option<blake3::Hasher>,
}

impl<W> Blake3Writer<W> {
    pub fn new(inner: W, enabled: bool) -> Self {
        Self {
            inner,
            blake3: enabled.then(blake3::Hasher::new),
        }
    }

    /// Consumes the writer and returns `(inner, optional_blake3_hex)`.
    pub fn finalize(self) -> (W, Option<String>) {
        let blake3_hex = self.blake3.map(|h| h.finalize().to_hex().to_string());
        (self.inner, blake3_hex)
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for Blake3Writer<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_write(cx, buf) {
            Poll::Ready(Ok(n)) => {
                if let Some(b3) = &mut this.blake3 {
                    b3.update(&buf[..n]);
                }
                Poll::Ready(Ok(n))
            }
            other => other,
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn test_blake3_writer_computes_hash() {
        let payload = b"hello pixeldrain blake3";
        let mut writer = Blake3Writer::new(Vec::new(), true);
        writer.write_all(&payload[..6]).await.unwrap();
        writer.write_all(&payload[6..]).await.unwrap();
        writer.flush().await.unwrap();

        let (written, b3_hex) = writer.finalize();
        assert_eq!(written, payload);
        let expected_b3 = blake3::hash(payload).to_hex().to_string();
        assert_eq!(b3_hex.as_deref(), Some(expected_b3.as_str()));
    }
}
