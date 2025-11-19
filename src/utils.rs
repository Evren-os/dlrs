use anyhow::{Context, Result};
use regex::Regex;
use std::path::PathBuf;
use std::sync::LazyLock;
use url::Url;

static DANGEROUS_CHARS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"[<>:"/\\|?*]"#).expect("Invalid regex"));

pub fn validate_url(raw_url: &str) -> Result<()> {
    if raw_url.is_empty() {
        anyhow::bail!("URL cannot be empty");
    }
    let u = Url::parse(raw_url).context("Invalid URL format")?;

    match u.scheme() {
        "http" | "https" | "ftp" => {}
        s => anyhow::bail!(
            "Unsupported URL scheme: {} (supported: http, https, ftp)",
            s
        ),
    }

    if u.host_str().is_none() {
        anyhow::bail!("URL must contain a host");
    }

    Ok(())
}

pub fn sanitize_filename(filename: &str) -> String {
    let mut name = DANGEROUS_CHARS_RE.replace_all(filename, "_").to_string();
    name = name.trim_matches(&[' ', '.'][..]).to_string();

    if name.is_empty() || is_reserved_name(&name) {
        let now = chrono::Local::now();
        return format!("download_{}", now.format("%Y%m%d_%H%M%S"));
    }

    name
}

fn is_reserved_name(name: &str) -> bool {
    let reserved = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let upper = name.to_uppercase();
    reserved.contains(&upper.as_str())
}

static ARIA2_PROGRESS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[#\w+\s+(\d+)B/(\d+)B\(\d+%\)").expect("Invalid regex"));

pub fn parse_aria2_progress(line: &str) -> Option<(u64, u64)> {
    let caps = ARIA2_PROGRESS_RE.captures(line)?;
    let downloaded = caps.get(1)?.as_str().parse().ok()?;
    let total = caps.get(2)?.as_str().parse().ok()?;
    Some((downloaded, total))
}

pub fn infer_filename_from_url(raw_url: &str) -> String {
    let u = match Url::parse(raw_url) {
        Ok(u) => u,
        Err(_) => {
            let now = chrono::Local::now();
            return format!("download_error_{}", now.format("%Y%m%d%H%M%S"));
        }
    };

    // Get path segments
    let path_segments: Vec<&str> = u.path_segments().map(|c| c.collect()).unwrap_or_default();

    let filename = if let Some(last) = path_segments.last() {
        last.to_string()
    } else {
        String::new()
    };

    if filename.is_empty() || filename == "." {
        if let Some(host) = u.host_str() {
            let name = sanitize_filename(host);
            let now = chrono::Local::now();
            return format!("download_from_{}_{}", name, now.format("%H%M%S"));
        }
        let now = chrono::Local::now();
        return format!("downloaded_file_{}", now.format("%Y%m%d_%H%M%S"));
    }

    sanitize_filename(&filename)
}

pub fn setup_destination(destination: Option<&String>) -> Result<PathBuf> {
    let target_dir = if let Some(dest) = destination {
        if dest.is_empty() {
            std::env::current_dir().context("Failed to get current directory")?
        } else {
            let p = PathBuf::from(dest);
            if let Ok(metadata) = std::fs::metadata(&p) {
                if !metadata.is_dir() {
                    anyhow::bail!("Destination must be a directory: {}", dest);
                }
                p.canonicalize().context("Failed to resolve path")?
            } else {
                // Create if not exists
                std::fs::create_dir_all(&p).context(format!("Creating directory '{}'", dest))?;
                p.canonicalize().context("Failed to resolve path")?
            }
        }
    } else {
        std::env::current_dir().context("Failed to get current directory")?
    };

    // Test write permissions
    let temp_file_path = target_dir.join(".dlrs-write-check");
    std::fs::write(&temp_file_path, "")
        .context(format!("Directory '{:?}' is not writable", target_dir))?;
    std::fs::remove_file(&temp_file_path).ok();

    Ok(target_dir)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("normal.txt"), "normal.txt");
        assert_eq!(sanitize_filename("fi:le?.txt"), "fi_le_.txt");
        assert_eq!(sanitize_filename("  spaces.txt  "), "spaces.txt");
        assert_eq!(sanitize_filename("CON"), sanitize_filename("CON"));
        assert!(sanitize_filename("CON").starts_with("download_"));
    }

    #[test]
    fn test_validate_url() {
        assert!(validate_url("https://google.com").is_ok());
        assert!(validate_url("ftp://example.com/file").is_ok());
        assert!(validate_url("invalid").is_err());
        assert!(validate_url("ssh://example.com").is_err());
    }

    #[test]
    fn test_infer_filename_from_url() {
        assert_eq!(
            infer_filename_from_url("https://example.com/file.zip"),
            "file.zip"
        );
        assert_eq!(
            infer_filename_from_url("https://example.com/path/to/file.tar.gz"),
            "file.tar.gz"
        );
        
        assert!(
            infer_filename_from_url("https://example.com/")
                .starts_with("download_from_example.com")
        );
    }

    #[test]
    fn test_parse_aria2_progress() {
        let line = "[#2089b0 1000B/2000B(50%) CN:1 DL:115KiB]";
        assert_eq!(parse_aria2_progress(line), Some((1000, 2000)));

        assert_eq!(parse_aria2_progress("Some random output"), None);

        assert_eq!(parse_aria2_progress("[#2089b0 1000B/"), None);
    }
}
