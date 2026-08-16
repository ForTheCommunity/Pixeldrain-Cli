use std::{
    pin::Pin,
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context, Poll},
    time::Duration,
};

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use tokio::io::{AsyncRead, ReadBuf};

// 2 bars are displayed simaltaneously.
pub struct UploadProgress {
    // first bar shows progress of a file.
    pub multi_pb: MultiProgress,
    // second bar shows overall progress of upload (tracks total bytes).
    pub overall_pb: ProgressBar,
    total_files: usize,
    files_done: AtomicUsize,
}

impl UploadProgress {
    // Create a new progress display for `total_files` uploads
    // with `total_bytes` across all files.
    pub fn new(total_files: usize, total_bytes: u64) -> Self {
        let multi_pb = MultiProgress::new();

        // Overall bar tracks TOTAL BYTES so ETA is based on upload speed.
        let overall_pb = multi_pb.add(ProgressBar::new(total_bytes));

        overall_pb.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}) ETA: {eta} | {msg}",
            )
            .unwrap()
            .progress_chars("█▉▊▇▆▅▃▂▁ "),
        );
        overall_pb.set_message(format!("0/{} files", total_files));
        overall_pb.enable_steady_tick(Duration::from_millis(100));

        Self {
            multi_pb,
            overall_pb,
            total_files,
            files_done: AtomicUsize::new(0),
        }
    }

    // Progress Bar per File.
    pub fn file_started(&self, filename: &str, file_size: u64) -> ProgressBar {
        let progress_bar = self
            .multi_pb
            .insert_before(&self.overall_pb, ProgressBar::new(file_size));

        progress_bar.set_style(
            ProgressStyle::with_template(
                "  ↳ {msg} [{wide_bar:.magenta/dim.magenta}] {bytes}/{total_bytes} ({bytes_per_sec}) ETA: {eta}",
            )
            .unwrap()
            .progress_chars("█▉▊▇▆▅▃▂▁ "),
        );

        progress_bar.set_message(filename.to_string());
        progress_bar.enable_steady_tick(Duration::from_millis(100));
        progress_bar
    }

    // cleanup when single file upload finishes.
    // clears per file progress bar, updates overall file count & prints status line.
    pub fn file_finished(&self, file_pb: &ProgressBar, filename: &str, success: bool) {
        file_pb.finish_and_clear();
        self.multi_pb.remove(file_pb);

        let done = self.files_done.fetch_add(1, Ordering::Relaxed) + 1;
        self.overall_pb
            .set_message(format!("{}/{} files", done, self.total_files));

        if success {
            self.overall_pb.println(format!("  ✓ {}", filename));
        } else {
            self.overall_pb.println(format!("  ✗ {}", filename));
        }
    }

    // when all uploads are done.
    pub fn all_finished(&self) {
        self.overall_pb.finish_with_message(format!(
            "  ✓ Upload Complete ({}/{} files)",
            self.files_done.load(Ordering::Relaxed),
            self.total_files
        ));
    }
}

// An `AsyncRead` wrapper that updates BOTH a per-file and overall
// `ProgressBar` as bytes are read.
pub struct ProgressReader<R> {
    inner: R,
    file_pb: ProgressBar,
    overall_pb: ProgressBar,
}
impl<R> ProgressReader<R> {
    pub fn new(inner: R, file_pb: ProgressBar, overall_pb: ProgressBar) -> Self {
        Self {
            inner,
            file_pb,
            overall_pb,
        }
    }
}
impl<R: AsyncRead + Unpin> AsyncRead for ProgressReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let result = Pin::new(&mut self.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &result {
            let bytes_read = (buf.filled().len() - before) as u64;
            // increase both bars by the number of bytes just read.
            self.file_pb.inc(bytes_read);
            self.overall_pb.inc(bytes_read);
        }
        result
    }
}
