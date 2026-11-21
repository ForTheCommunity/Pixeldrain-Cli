use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::storage::project_data_dir;

pub const GITLAB_TAGS_URL: &str = "https://gitlab.com/api/v4/projects/85434766/repository/tags";
pub const GITHUB_TAGS_URL: &str =
    "https://api.github.com/repos/ForTheCommunity/pixeldrain-cli/tags";

pub const GITHUB_CHANGELOG_URL: &str =
    "https://raw.githubusercontent.com/ForTheCommunity/Pixeldrain-Cli/refs/heads/main/CHANGELOG.md";
pub const GITLAB_CHANGELOG_URL: &str =
    "https://gitlab.com/ForTheCommunity/pixeldrain-cli/-/raw/main/CHANGELOG.md?ref_type=heads";

pub const GITLAB_RELEASES_URL: &str =
    "https://gitlab.com/ForTheCommunity/pixeldrain-cli/-/releases";
pub const GITHUB_RELEASES_URL: &str = "https://github.com/ForTheCommunity/pixeldrain-cli/releases";

#[derive(Debug, Clone, Deserialize)]
pub struct TagItem {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCache {
    pub last_checked_epoch_secs: u64,
    pub latest_version: String,
}

/// Fetches repository tags, trying GitLab first with automatic fallback to GitHub.
pub async fn fetch_tags(client: &reqwest::Client) -> Result<(Vec<TagItem>, &'static str)> {
    // Try GitLab tags endpoint
    match client.get(GITLAB_TAGS_URL).send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(tags) = resp.json::<Vec<TagItem>>().await {
                if !tags.is_empty() {
                    return Ok((tags, "GitLab"));
                }
            }
        }
        _ => {}
    }

    // Fallback to GitHub tags endpoint
    match client.get(GITHUB_TAGS_URL).send().await {
        Ok(resp) if resp.status().is_success() => {
            let tags = resp.json::<Vec<TagItem>>().await?;
            if !tags.is_empty() {
                return Ok((tags, "GitHub"));
            }
            bail!("No tags found in GitHub repository");
        }
        Ok(resp) => {
            bail!("GitHub tags API returned HTTP {}", resp.status());
        }
        Err(e) => {
            bail!("Failed to fetch tags from GitLab and GitHub: {e}");
        }
    }
}

/// Fetches CHANGELOG.md, trying GitHub first with automatic fallback to GitLab.
pub async fn fetch_changelog(client: &reqwest::Client) -> Result<(String, &'static str)> {
    // Try GitHub raw changelog
    match client.get(GITHUB_CHANGELOG_URL).send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(text) = resp.text().await {
                if !text.trim().is_empty() {
                    return Ok((text, "GitHub"));
                }
            }
        }
        _ => {}
    }

    // Fallback to GitLab raw changelog
    match client.get(GITLAB_CHANGELOG_URL).send().await {
        Ok(resp) if resp.status().is_success() => {
            let text = resp.text().await?;
            if !text.trim().is_empty() {
                return Ok((text, "GitLab"));
            }
            bail!("Changelog is empty");
        }
        Ok(resp) => {
            bail!("GitLab changelog returned HTTP {}", resp.status());
        }
        Err(e) => {
            bail!("Failed to fetch changelog from GitHub and GitLab: {e}");
        }
    }
}

/// Parses the highest semantic version from the list of repository tags.
pub fn parse_latest_version(tags: &[TagItem]) -> Option<semver::Version> {
    tags.iter()
        .filter_map(|t| {
            let clean = t.name.trim().trim_start_matches('v');
            semver::Version::parse(clean).ok()
        })
        .max()
}

/// Extracts the changelog section corresponding to a specific version.
/// If not found, falls back to the top release section in the changelog.
pub fn extract_changelog_for_version(
    changelog_md: &str,
    version: &semver::Version,
) -> Option<String> {
    let v_str = version.to_string();
    let lines: Vec<&str> = changelog_md.lines().collect();

    // Look for a heading line starting with "## " that references this version
    let mut start_idx = None;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            let heading_rest = trimmed.trim_start_matches('#').trim();
            if heading_rest.contains(&v_str) {
                start_idx = Some(i);
                break;
            }
        }
    }

    // If exact version was not found, fall back to the first release section (##)
    let start = match start_idx {
        Some(idx) => idx,
        None => lines.iter().position(|l| l.trim().starts_with("## "))?,
    };

    // Collect until the next "## " line (ignoring sub-headings like "### Added")
    let mut end = lines.len();
    for (i, line) in lines.iter().enumerate().skip(start + 1) {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            end = i;
            break;
        }
    }

    let section = lines[start..end].join("\n");
    let trimmed = section.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn cache_path() -> Result<std::path::PathBuf> {
    Ok(project_data_dir()?.join("update_cache.json"))
}

pub fn load_update_cache() -> Option<UpdateCache> {
    let path = cache_path().ok()?;
    if !path.exists() {
        return None;
    }
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

pub fn save_update_cache(cache: &UpdateCache) -> Result<()> {
    let path = cache_path()?;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(cache)?;
    std::fs::write(path, json)?;
    Ok(())
}

fn current_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn render_update_box(
    current: &semver::Version,
    latest: &semver::Version,
    source: &str,
) -> String {
    let cyan = "\x1b[1;36m";
    let green = "\x1b[1;32m";
    let bold = "\x1b[1m";
    let dim = "\x1b[90m";
    let reset = "\x1b[0m";

    let width: usize = 65;
    let line1_vis = "A new version of pixeldrain-cli is available!";
    let line2_vis = format!("Current version : v{}", current);
    let line3_vis = format!("Latest version  : v{} (via {})", latest, source);

    let pad1 = " ".repeat(width.saturating_sub(line1_vis.len()));
    let pad2 = " ".repeat(width.saturating_sub(line2_vis.len()));
    let pad3 = " ".repeat(width.saturating_sub(line3_vis.len()));

    format!(
        "\n   {cyan}┌───── Update Available ────────────────────────────────────────────┐{reset}\n   \
           {cyan}│{reset}  A new version of {bold}pixeldrain-cli{reset} is available!{pad1}{cyan}│{reset}\n   \
           {cyan}│{reset}  Current version : {dim}v{current}{reset}{pad2}{cyan}│{reset}\n   \
           {cyan}│{reset}  Latest version  : {green}v{latest}{reset} {dim}(via {source}){reset}{pad3}{cyan}│{reset}\n   \
           {cyan}└───────────────────────────────────────────────────────────────────┘{reset}\n"
    )
}

pub fn render_changelog_box(
    version: &semver::Version,
    changelog_notes: &str,
    source: &str,
) -> String {
    let cyan = "\x1b[1;36m";
    let bold = "\x1b[1m";
    let dim = "\x1b[90m";
    let reset = "\x1b[0m";

    let rule = "─".repeat(67);
    let mut out = String::new();
    out.push_str(&format!(
        "   {bold}What's new in v{version}{reset} {dim}(via {source}){reset}:\n"
    ));
    out.push_str(&format!("   {cyan}{rule}{reset}\n"));
    for line in changelog_notes.lines() {
        out.push_str(&format!("   {}\n", line));
    }
    out.push_str(&format!("   {cyan}{rule}{reset}\n\n"));
    out.push_str("   To update pixeldrain-cli:\n");
    out.push_str(&format!(
        "     {bold}• Cargo     :{reset} cargo install pixeldrain-cli --force\n"
    ));
    out.push_str(&format!(
        "     {bold}• AM/AppMan :{reset} am -u pixeldrain-cli (or appman -u pixeldrain-cli)\n"
    ));
    out.push_str(&format!(
        "     {bold}• GitLab    :{reset} {}\n",
        GITLAB_RELEASES_URL
    ));
    out.push_str(&format!(
        "     {bold}• GitHub    :{reset} {}\n",
        GITHUB_RELEASES_URL
    ));
    out
}

/// Checks for updates immediately, prints progress, compares versions,
/// and displays the changelog / what's new for the latest version.
pub async fn check_for_updates(show_changelog_always: bool) -> Result<()> {
    println!("🔍 Checking for updates...");

    let client = reqwest::Client::builder()
        .user_agent("pixeldrain-cli")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| anyhow!("Failed to initialize HTTP client: {e}"))?;

    let (tags, source) = fetch_tags(&client).await?;
    let latest_version = parse_latest_version(&tags)
        .ok_or_else(|| anyhow!("No valid semantic versions found in tags from {source}"))?;

    let current_pkg_version = env!("CARGO_PKG_VERSION");
    let current_version = semver::Version::parse(current_pkg_version)
        .map_err(|e| anyhow!("Failed to parse current version '{current_pkg_version}': {e}"))?;

    // Update cache
    let _ = save_update_cache(&UpdateCache {
        last_checked_epoch_secs: current_epoch_secs(),
        latest_version: latest_version.to_string(),
    });

    if latest_version > current_version {
        print!(
            "{}",
            render_update_box(&current_version, &latest_version, source)
        );

        // Fetch and show changelog
        match fetch_changelog(&client).await {
            Ok((changelog_md, cl_source)) => {
                if let Some(notes) = extract_changelog_for_version(&changelog_md, &latest_version) {
                    println!(
                        "{}",
                        render_changelog_box(&latest_version, &notes, cl_source)
                    );
                }
            }
            Err(e) => {
                println!("   (Could not load changelog: {e})\n");
                println!("   To update pixeldrain-cli:");
                println!("     • Cargo     : cargo install pixeldrain-cli --force");
                println!("     • AM/AppMan : am -u pixeldrain-cli (or appman -u pixeldrain-cli)");
                println!("     • GitLab    : {}", GITLAB_RELEASES_URL);
                println!("     • GitHub    : {}", GITHUB_RELEASES_URL);
            }
        }
    } else if latest_version == current_version {
        println!("   Current version : v{}", current_version);
        println!("   Latest version  : v{} (via {})", latest_version, source);
        println!();
        println!("✓ You are running the latest version of pixeldrain-cli.");
        if show_changelog_always {
            println!();
            match fetch_changelog(&client).await {
                Ok((changelog_md, cl_source)) => {
                    if let Some(notes) =
                        extract_changelog_for_version(&changelog_md, &latest_version)
                    {
                        println!(
                            "{}",
                            render_changelog_box(&latest_version, &notes, cl_source)
                        );
                    }
                }
                Err(e) => {
                    println!("(Could not load changelog: {e})");
                }
            }
        }
    } else {
        println!("   Current version : v{}", current_version);
        println!("   Latest version  : v{} (via {})", latest_version, source);
        println!();
        println!(
            "You are running a development or pre-release build (v{} > v{}).",
            current_version, latest_version
        );
    }

    Ok(())
}

/// Checks the local cache for any known newer version and returns a non-intrusive notification.
/// This does not block or execute network calls during normal commands.
pub fn get_cached_update_notice() -> Option<String> {
    // Allow users to suppress update notices
    if std::env::var("PIXELDRAIN_NO_UPDATE_CHECK")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        return None;
    }

    let cache = load_update_cache()?;
    let latest = semver::Version::parse(&cache.latest_version).ok()?;
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION")).ok()?;

    if latest > current {
        Some(format!(
            "💡 A new version of pixeldrain-cli is available: v{} -> v{}. Run 'pixeldrain-cli check-update' to see what's new.",
            current, latest
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_latest_version() {
        let tags = vec![
            TagItem {
                name: "v0.1.1".to_string(),
            },
            TagItem {
                name: "0.1.4".to_string(),
            },
            TagItem {
                name: "v0.1.10".to_string(),
            },
            TagItem {
                name: "v0.1.5".to_string(),
            },
            TagItem {
                name: "invalid-tag".to_string(),
            },
        ];

        let latest = parse_latest_version(&tags).unwrap();
        assert_eq!(latest, semver::Version::parse("0.1.10").unwrap());
    }

    #[test]
    fn test_extract_changelog_for_version() {
        let changelog = r#"# Changelog

## [0.1.5] - 2026-10-11
- Decoupled old messy monolithic code
- Improved error handling

## [0.1.4] - 2026-08-25
- added feature to Move Uploaded files
"#;

        let v015 = semver::Version::parse("0.1.5").unwrap();
        let section = extract_changelog_for_version(changelog, &v015).unwrap();
        assert!(section.contains("## [0.1.5] - 2026-10-11"));
        assert!(section.contains("Decoupled old messy monolithic code"));
        assert!(!section.contains("0.1.4"));

        // Fallback to latest available if version not present
        let v016 = semver::Version::parse("0.1.6").unwrap();
        let fallback = extract_changelog_for_version(changelog, &v016).unwrap();
        assert!(fallback.contains("## [0.1.5] - 2026-10-11"));
    }

    #[test]
    fn test_semver_comparison_logic() {
        let v_old = semver::Version::parse("0.1.5").unwrap();
        let v_new = semver::Version::parse("0.1.6").unwrap();
        assert!(v_new > v_old);
    }
}
