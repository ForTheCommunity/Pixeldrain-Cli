use std::{
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use tokio::io::{AsyncRead, ReadBuf};

// Progress manager for single and multi-file uploads.
#[derive(Clone)]
pub struct UploadProgress {
    pub multi_pb: MultiProgress,
    pub overall_pb: ProgressBar,
    pub total_files: usize,
    files_done: Arc<AtomicUsize>,
}

impl UploadProgress {
    // Create a new progress display for `total_files` uploads
    // with `total_bytes` across all files.
    pub fn new(total_files: usize, total_bytes: u64) -> Self {
        let multi_pb = MultiProgress::new();
        let overall_pb = multi_pb.add(ProgressBar::new(total_bytes));

        if total_files == 1 {
            overall_pb.set_style(
                ProgressStyle::with_template(
                    "  ↳ {msg} [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}) ETA: {eta}",
                )
                .unwrap()
                .progress_chars("=>-"),
            );
        } else {
            overall_pb.set_style(
                ProgressStyle::with_template(
                    "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}) ETA: {eta} | {msg}",
                )
                .unwrap()
                .progress_chars("=>-"),
            );
            overall_pb.set_message(format!("0/{} files", total_files));
        }

        overall_pb.enable_steady_tick(Duration::from_millis(100));

        Self {
            multi_pb,
            overall_pb,
            total_files,
            files_done: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn is_single_file(&self) -> bool {
        self.total_files == 1
    }

    // Progress Bar per File.
    // If uploading only 1 file, re-uses overall_pb so only 1 progress bar is rendered.
    pub fn file_started(&self, filename: &str, file_size: u64) -> ProgressBar {
        let safe_name = truncate_filename(filename, 30);

        if self.is_single_file() {
            self.overall_pb.set_message(safe_name);
            self.overall_pb.clone()
        } else {
            let progress_bar = self
                .multi_pb
                .insert_before(&self.overall_pb, ProgressBar::new(file_size));

            progress_bar.set_style(
                ProgressStyle::with_template(
                    "  ↳ {msg} [{wide_bar:.magenta/dim.magenta}] {bytes}/{total_bytes} ({bytes_per_sec}) ETA: {eta}",
                )
                .unwrap()
                .progress_chars("=>-"),
            );

            progress_bar.set_message(safe_name);
            progress_bar.enable_steady_tick(Duration::from_millis(100));
            progress_bar
        }
    }

    // cleanup when single file upload finishes.
    pub fn file_finished(&self, file_pb: &ProgressBar, filename: &str, success: bool) {
        if !self.is_single_file() {
            file_pb.finish_and_clear();
            self.multi_pb.remove(file_pb);

            let done = self.files_done.fetch_add(1, Ordering::Relaxed) + 1;
            self.overall_pb
                .set_message(format!("{}/{} files", done, self.total_files));
        } else {
            self.files_done.fetch_add(1, Ordering::Relaxed);
        }

        if success {
            self.overall_pb.println(format!("  ✓ {}", filename));
        } else {
            self.overall_pb.println(format!("  ✗ {}", filename));
        }
    }

    // when all uploads are done.
    pub fn all_finished(&self) {
        if self.is_single_file() {
            self.overall_pb.finish_with_message("  ✓ Upload Complete");
        } else {
            self.overall_pb.finish_with_message(format!(
                "  ✓ Upload Complete ({}/{} files)",
                self.files_done.load(Ordering::Relaxed),
                self.total_files
            ));
        }
    }
}

// An `AsyncRead` wrapper that updates progress bar(s) as bytes are read.
pub struct ProgressReader<R> {
    inner: R,
    file_pb: ProgressBar,
    overall_pb: ProgressBar,
    is_multi: bool,
}

impl<R> ProgressReader<R> {
    pub fn new(inner: R, file_pb: ProgressBar, overall_pb: ProgressBar, is_multi: bool) -> Self {
        Self {
            inner,
            file_pb,
            overall_pb,
            is_multi,
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
            self.file_pb.inc(bytes_read);
            if self.is_multi {
                self.overall_pb.inc(bytes_read);
            }
        }
        result
    }
}

// truncate long filenames
fn truncate_filename(name: &str, max_len: usize) -> String {
    if name.chars().count() <= max_len {
        name.to_string()
    } else {
        let truncated: String = name.chars().take(max_len - 3).collect();
        format!("{}...", truncated)
    }
}
