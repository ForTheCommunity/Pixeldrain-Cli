use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

pub fn collect_files(
    paths: &[impl AsRef<Path>],
    formats: Option<&[String]>,
) -> Result<Vec<PathBuf>> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }

    let allowed_formats: Option<HashSet<String>> = formats.map(|fmts| {
        fmts.iter()
            .map(|f| f.trim_start_matches('.').to_ascii_lowercase())
            .collect()
    });

    let mut collected_files = Vec::new();
    let mut visited_dirs = HashSet::new();

    for raw_path in paths {
        let path = raw_path.as_ref();

        if !path.exists() {
            bail!("Path does not exist: {}", path.display());
        }

        // Canonicalize to resolve relative parts and symlinks
        let canonical = path
            .canonicalize()
            .with_context(|| format!("Failed to resolve path: {}", path.display()))?;

        if canonical.is_file() {
            if matches_format(&canonical, allowed_formats.as_ref()) {
                collected_files.push(canonical);
            }
        } else if canonical.is_dir() {
            collect_dir_recursive(
                &canonical,
                allowed_formats.as_ref(),
                &mut collected_files,
                &mut visited_dirs,
            )?;
        } else {
            bail!(
                "Path is neither a regular file nor a directory: {}",
                path.display()
            );
        }
    }

    // Natural alphanumeric sorting and deduplication
    normalize_paths(&mut collected_files);

    Ok(collected_files)
}

fn collect_dir_recursive(
    directory: &Path,
    formats: Option<&HashSet<String>>,
    files: &mut Vec<PathBuf>,
    visited_dirs: &mut HashSet<PathBuf>,
) -> Result<()> {
    // Prevent infinite loops from symlink cycles
    if !visited_dirs.insert(directory.to_path_buf()) {
        return Ok(());
    }

    let entries = fs::read_dir(directory)
        .with_context(|| format!("Failed to read directory: {}", directory.display()))?;

    for entry in entries {
        let entry = entry.with_context(|| {
            format!(
                "Failed to access directory entry in: {}",
                directory.display()
            )
        })?;
        let path = entry.path();

        // Query metadata (follows symlinks if needed)
        let file_type = entry
            .file_type()
            .with_context(|| format!("Failed to read file type for: {}", path.display()))?;

        if file_type.is_file() {
            if matches_format(&path, formats) {
                files.push(path);
            }
        } else if file_type.is_dir() {
            // Resolve canonical path for subdirectory
            if let Ok(canonical_sub_dir) = path.canonicalize() {
                collect_dir_recursive(&canonical_sub_dir, formats, files, visited_dirs)?;
            }
        }
    }

    Ok(())
}

fn matches_format(path: &Path, formats: Option<&HashSet<String>>) -> bool {
    let Some(formats) = formats else {
        return true;
    };

    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| formats.contains(&ext.to_ascii_lowercase()))
        .unwrap_or(false)
}

fn normalize_paths(files: &mut Vec<PathBuf>) {
    // Natural alphanumeric ordering
    files.sort_by(|a, b| natord::compare(&a.to_string_lossy(), &b.to_string_lossy()));

    // Deduplicate identical canonical paths
    files.dedup();
}
