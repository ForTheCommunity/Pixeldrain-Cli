use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use tokio::io::{AsyncRead, ReadBuf};

// 2 bars are displayed simaltaneously.
pub struct UploadProgress {
    // first bar shows progress of a file.
    pub multi_pb: MultiProgress,
    // second bar shows overall progress of upload.
    pub overall_pb: ProgressBar,
}

impl UploadProgress {
    // Create a new progress display for `total_files` uploads.
    pub fn new(total_files: usize) -> Self {
        let multi_pb = MultiProgress::new();
        let overall_pb = multi_pb.add(ProgressBar::new(total_files as u64));

        overall_pb.set_style(
            ProgressStyle::with_template(
                "
{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} files uploaded
                ",
            )
            .unwrap()
            .progress_chars("█▉▊▇▆▅▃▂▁ "),
        );
        overall_pb.enable_steady_tick(Duration::from_millis(100));
        Self {
            multi_pb,
            overall_pb,
        }
    }

    // Progress Bar per File.
    pub fn file_started(&self, filename: &str, file_size: u64) -> ProgressBar {
        let progress_bar = self
            .multi_pb
            .insert_before(&self.overall_pb, ProgressBar::new(file_size));

        progress_bar.set_style(
            ProgressStyle::with_template("  ↳ {msg} [{wide_bar:.magenta/dim.magenta}] {bytes}/{total_bytes} ({bytes_per_sec})").unwrap().progress_chars("█▉▊▇▆▅▃▂▁ ")
        );

        progress_bar.set_message(filename.to_string());
        progress_bar.enable_steady_tick(Duration::from_millis(100));
        progress_bar
    }

    // cleanup when single file upload finishes.
    // clears per file progress bar, increments overall counter & prints status line.
    pub fn file_finished(&self, file_pb: &ProgressBar, filename: &str, success: bool) {
        file_pb.finish_and_clear();
        self.multi_pb.remove(file_pb);
        self.overall_pb.inc(1);
        if success {
            self.overall_pb.println(format!("  ✓ {}", filename));
        } else {
            self.overall_pb.println(format!("  ✗ {}", filename));
        }
    }

    // when all uploads are done.
    pub fn all_finished(&self) {
        self.overall_pb.finish_with_message("  ✓ Upload Complete");
    }
}

// An `AsyncRead` wrapper that updates an `indicatif::ProgressBar` as bytes are read.
pub struct ProgressReader<R> {
    inner: R,
    progress: ProgressBar,
}
impl<R> ProgressReader<R> {
    pub fn new(inner: R, progress: ProgressBar) -> Self {
        Self { inner, progress }
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
            let after = buf.filled().len();
            self.progress.inc((after - before) as u64);
        }
        result
    }
}
