use crate::error::DlrsError;
use regex::Regex;
use std::path::PathBuf;
use std::sync::LazyLock;
use url::Url;

static DANGEROUS_CHARS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"[<>:"/\\|?*]"#).expect("Invalid regex"));

static ARIA2_PROGRESS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[#\w+\s+(\d+)B/(\d+)B\(\d+%\)").expect("Invalid regex"));

pub fn validate_url(raw_url: &str) -> Result<(), DlrsError> {
    if raw_url.is_empty() {
        return Err(DlrsError::InvalidUrl("URL cannot be empty".into()));
    }

    let u = Url::parse(raw_url).map_err(|_| DlrsError::InvalidUrl("Invalid format".into()))?;

    match u.scheme() {
        "http" | "https" | "ftp" => {}
        s => {
            return Err(DlrsError::InvalidUrl(format!(
                "Unsupported scheme: {s} (use http/https/ftp)"
            )));
        }
    }

    if u.host_str().is_none() {
        return Err(DlrsError::InvalidUrl("Missing host".into()));
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
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    RESERVED.contains(&name.to_uppercase().as_str())
}

pub fn parse_aria2_progress(line: &str) -> Option<(u64, u64)> {
    let caps = ARIA2_PROGRESS_RE.captures(line)?;
    let downloaded = caps.get(1)?.as_str().parse().ok()?;
    let total = caps.get(2)?.as_str().parse().ok()?;
    Some((downloaded, total))
}

pub fn infer_filename_from_url(raw_url: &str) -> String {
    let Ok(u) = Url::parse(raw_url) else {
        let now = chrono::Local::now();
        return format!("download_error_{}", now.format("%Y%m%d%H%M%S"));
    };

    let path_segments: Vec<&str> = u.path_segments().map(|c| c.collect()).unwrap_or_default();

    let filename = path_segments
        .last()
        .map(|s| s.to_string())
        .unwrap_or_default();

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

pub fn setup_destination(destination: Option<&String>) -> Result<PathBuf, DlrsError> {
    let target_dir = match destination {
        Some(dest) if !dest.is_empty() => {
            let p = PathBuf::from(dest);
            if let Ok(meta) = std::fs::metadata(&p) {
                if !meta.is_dir() {
                    return Err(DlrsError::DestinationError(format!(
                        "Not a directory: {dest}"
                    )));
                }
                p.canonicalize()
                    .map_err(|_| DlrsError::DestinationError("Failed to resolve path".into()))?
            } else {
                std::fs::create_dir_all(&p).map_err(|_| {
                    DlrsError::DestinationError(format!("Failed to create directory: {dest}"))
                })?;
                p.canonicalize()
                    .map_err(|_| DlrsError::DestinationError("Failed to resolve path".into()))?
            }
        }
        _ => std::env::current_dir()
            .map_err(|_| DlrsError::DestinationError("Failed to get current directory".into()))?,
    };

    let temp_file = target_dir.join(".dlrs-write-check");
    std::fs::write(&temp_file, "").map_err(|_| {
        DlrsError::DestinationError(format!("Directory not writable: {target_dir:?}"))
    })?;
    let _ = std::fs::remove_file(&temp_file);

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
        assert!(infer_filename_from_url("https://example.com/").starts_with("download_from_"));
    }

    #[test]
    fn test_parse_aria2_progress() {
        let line = "[#2089b0 1000B/2000B(50%) CN:1 DL:115KiB]";
        assert_eq!(parse_aria2_progress(line), Some((1000, 2000)));
        assert_eq!(parse_aria2_progress("Some random output"), None);
        assert_eq!(parse_aria2_progress("[#2089b0 1000B/"), None);
    }
}
