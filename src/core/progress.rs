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
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

#[derive(Clone, Copy)]
pub enum TransferType {
    Upload,
    Download,
}

impl TransferType {
    fn label(&self) -> &'static str {
        match self {
            Self::Upload => "Upload",
            Self::Download => "Download",
        }
    }

    fn success_symbol(&self) -> &'static str {
        match self {
            Self::Upload => "✓",
            Self::Download => "⬇",
        }
    }

    fn failure_symbol(&self) -> &'static str {
        "✗"
    }

    // Progress Bar Color for the single-file / overall bar.
    fn overall_bar_colors(&self) -> &'static str {
        match self {
            Self::Upload => ".cyan/blue",
            Self::Download => ".green/dim.green",
        }
    }

    ///  per-file bars Progress Bar Color in multi-file mode.
    fn per_file_bar_colors(&self) -> &'static str {
        match self {
            Self::Upload => ".magenta/dim.magenta",
            Self::Download => ".green/dim.green",
        }
    }
}

// Transfer Progress Manager
#[derive(Clone)]
pub struct TransferProgress {
    transfer_type: TransferType,
    pub multi_pb: MultiProgress,
    pub overall_pb: ProgressBar,
    pub total_files: usize,
    files_done: Arc<AtomicUsize>,
}

impl TransferProgress {
    // Creates a new progress bar/s.
    pub fn new(transfer_type: TransferType, total_files: usize, total_bytes: u64) -> Self {
        let multi_pb = MultiProgress::new();
        let overall_pb = multi_pb.add(ProgressBar::new(total_bytes));

        let bar_colors = transfer_type.overall_bar_colors();

        if total_files == 1 {
            overall_pb.set_style(
                ProgressStyle::with_template(&format!(
                    "  ↳ {{msg}} [{{wide_bar:{bar_colors}}}] {{bytes}}/{{total_bytes}} ({{bytes_per_sec}}) ETA: {{eta}}"
                ))
                .unwrap()
                .progress_chars("=>-"),
            );
        } else {
            overall_pb.set_style(
                ProgressStyle::with_template(&format!(
                    "{{spinner:.green}} [{{elapsed_precise}}] [{{wide_bar:{bar_colors}}}] {{bytes}}/{{total_bytes}} ({{bytes_per_sec}}) ETA: {{eta}} | {{msg}}"
                ))
                .unwrap()
                .progress_chars("=>-"),
            );
            overall_pb.set_message(format!("0/{} files", total_files));
        }

        overall_pb.enable_steady_tick(Duration::from_millis(100));

        Self {
            transfer_type,
            multi_pb,
            overall_pb,
            total_files,
            files_done: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn is_single_file(&self) -> bool {
        self.total_files == 1
    }

    // Progress Bar per file.
    // if transferring only 1 file then only one pb is shown.
    pub fn file_started(&self, filename: &str, file_size: u64) -> ProgressBar {
        let safe_name = truncate_filename(filename, 30);

        if self.is_single_file() {
            self.overall_pb.set_message(safe_name);
            self.overall_pb.clone()
        } else {
            let bar_colors = self.transfer_type.per_file_bar_colors();

            let progress_bar = self
                .multi_pb
                .insert_before(&self.overall_pb, ProgressBar::new(file_size));

            progress_bar.set_style(
                ProgressStyle::with_template(&format!(
                    "  ↳ {{msg}} [{{wide_bar:{bar_colors}}}] {{bytes}}/{{total_bytes}} ({{bytes_per_sec}}) ETA: {{eta}}"
                ))
                .unwrap()
                .progress_chars("=>-"),
            );

            progress_bar.set_message(safe_name);
            progress_bar.enable_steady_tick(Duration::from_millis(100));
            progress_bar
        }
    }

    // Cleanup when a single file transfer finishes.
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

        let icon = if success {
            self.transfer_type.success_symbol()
        } else {
            self.transfer_type.failure_symbol()
        };
        self.overall_pb.println(format!("  {} {}", icon, filename));
    }

    // Cleanup when a single file transfer finishes.
    // used for file uploads...
    pub fn file_finished_with_url(
        &self,
        file_pb: &ProgressBar,
        file_name: &str,
        success: bool,
        url: Option<&str>,
    ) {
        let done = self.files_done.fetch_add(1, Ordering::Relaxed) + 1;

        if !self.is_single_file() {
            file_pb.finish_and_clear();
            self.multi_pb.remove(file_pb);
            self.overall_pb
                .set_message(format!("{}/{} files", done, self.total_files));
        }

        let status_str = if success {
            if let Some(u) = url {
                format!("  ✓  {}  --> {}", file_name, u)
            } else {
                format!("  ✓  {}", file_name)
            }
        } else {
            format!("  ✗  {}", file_name)
        };

        self.overall_pb.println(status_str);
    }

    // When all transfers are done.
    pub fn all_finished(&self) {
        let label = self.transfer_type.label();

        if self.is_single_file() {
            self.overall_pb
                .finish_with_message(format!("  ✓ {} Complete", label));
        } else {
            self.overall_pb.finish_with_message(format!(
                "  ✓ {} Complete ({}/{} files)",
                label,
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

// An `AsyncWrite` wrapper that updates progress bar(s) as bytes are written.
pub struct ProgressWriter<W> {
    inner: W,
    file_pb: ProgressBar,
    overall_pb: ProgressBar,
    is_multi: bool,
}

impl<W> ProgressWriter<W> {
    pub fn new(inner: W, file_pb: ProgressBar, overall_pb: ProgressBar, is_multi: bool) -> Self {
        Self {
            inner,
            file_pb,
            overall_pb,
            is_multi,
        }
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for ProgressWriter<W> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let result = Pin::new(&mut self.inner).poll_write(cx, buf);
        if let Poll::Ready(Ok(bytes_written)) = &result {
            self.file_pb.inc(*bytes_written as u64);
            if self.is_multi {
                self.overall_pb.inc(*bytes_written as u64);
            }
        }
        result
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

// Truncate long filenames.
fn truncate_filename(name: &str, max_len: usize) -> String {
    if name.chars().count() <= max_len {
        name.to_string()
    } else {
        let truncated: String = name.chars().take(max_len - 3).collect();
        format!("{}...", truncated)
    }
}
