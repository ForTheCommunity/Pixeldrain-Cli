pub mod collector;
pub mod crypto;
pub mod hash;
pub mod login;
pub mod password;
pub mod progress;
pub mod state;
pub mod storage;

/// Maximum single-file upload size (10 GiB) processed without splitting.
pub const MAX_UPLOAD_FILE_SIZE: u64 = 10 * 1024 * 1024 * 1024;

/// Returns `true` if `file_size` exceeds [`MAX_UPLOAD_FILE_SIZE`].
pub fn exceeds_max_upload_size(file_size: u64) -> bool {
    file_size > MAX_UPLOAD_FILE_SIZE
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];

    let mut value = bytes as f64;
    let mut unit = 0usize;

    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{:.2} {}", value, UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exceeds_max_upload_size() {
        assert!(!exceeds_max_upload_size(0));
        assert!(!exceeds_max_upload_size(MAX_UPLOAD_FILE_SIZE));
        assert!(exceeds_max_upload_size(MAX_UPLOAD_FILE_SIZE + 1));
    }
}
