use crate::cli::Cli;
use crate::utils::{infer_filename_from_url, sanitize_filename};
use anyhow::{Context, Result};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use regex::Regex;
use reqwest::header::CONTENT_DISPOSITION;
use std::path::Path;
use std::process::Stdio;
use std::sync::LazyLock;
use std::time::Duration;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

static CONTENT_DISPOSITION_FILENAME_STAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"filename\*\s*=\s*([^;]+)").expect("Invalid regex"));
static CONTENT_DISPOSITION_FILENAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"filename\s*=\s*([^;]+)").expect("Invalid regex"));

pub struct DownloadItem {
    pub url: String,
    pub filename: String,
    pub file_path: String,
}

pub async fn detect_filename(
    url: &str,
    user_agent: Option<&str>,
    timeout_secs: u64,
) -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()?;

    let mut req = client.head(url);
    if let Some(ua) = user_agent {
        req = req.header("User-Agent", ua);
    } else {
        req = req.header("User-Agent", "dlrs/1.0");
    }

    let resp = req.send().await?;

    if let Some(name) = resp
        .headers()
        .get(CONTENT_DISPOSITION)
        .and_then(|cd| cd.to_str().ok())
        .and_then(parse_content_disposition)
    {
        return Ok(sanitize_filename(&name));
    }

    Ok(infer_filename_from_url(url))
}

fn parse_content_disposition(header: &str) -> Option<String> {
    if let Some(caps) = CONTENT_DISPOSITION_FILENAME_STAR.captures(header) {
        let encoded = caps.get(1)?.as_str().trim_matches(&['"', '\'', ' '][..]);
        if let Some(decoded) = decode_rfc5987(encoded) {
            return Some(decoded);
        }
    }

    if let Some(caps) = CONTENT_DISPOSITION_FILENAME.captures(header) {
        let filename = caps.get(1)?.as_str().trim_matches(&['"', '\'', ' '][..]);
        return Some(filename.to_string());
    }

    None
}

fn decode_rfc5987(encoded: &str) -> Option<String> {
    let parts: Vec<&str> = encoded.splitn(3, '\'').collect();
    if parts.len() != 3 {
        return None;
    }
    // parts[2] is the encoded filename
    url::form_urlencoded::parse(parts[2].as_bytes())
        .map(|(k, _)| k.to_string())
        .next()
}

pub fn build_aria2c_args(target_dir: &str, filename: &str, url: &str, config: &Cli) -> Vec<String> {
    let mut args = vec![
        format!("--dir={}", target_dir),
        format!("--out={}", filename),
        "--continue=true".to_string(),
        "--max-connection-per-server=8".to_string(),
        "--split=32".to_string(),
        "--min-split-size=1M".to_string(),
        "--file-allocation=falloc".to_string(),
        format!("--max-tries={}", config.max_tries),
        format!("--retry-wait={}", config.retry_wait),
        format!("--connect-timeout={}", config.connect_timeout),
        format!("--timeout={}", config.timeout),
        "--max-file-not-found=3".to_string(),
        "--summary-interval=1".to_string(),
        "--console-log-level=warn".to_string(),
        "--auto-file-renaming=false".to_string(),
        "--allow-overwrite=true".to_string(),
        "--conditional-get=true".to_string(),
        "--check-integrity=true".to_string(),
        "--disk-cache=128M".to_string(),
        "--async-dns=true".to_string(),
        "--http-accept-gzip=true".to_string(),
        "--remote-time=true".to_string(),
    ];

    if let Some(speed) = &config.max_speed {
        args.push(format!("--max-download-limit={}", speed));
    }

    if let Some(ua) = &config.user_agent {
        args.push(format!("--user-agent={}", ua));
    }

    args.push(url.to_string());
    args
}

pub async fn download_file(
    item: &mut DownloadItem,
    target_dir: &str,
    config: &Cli,
    mp: Option<&MultiProgress>,
    cancel_token: CancellationToken,
) -> Result<()> {
    let filename = match detect_filename(
        &item.url,
        config.user_agent.as_deref(),
        config.connect_timeout,
    )
    .await
    {
        Ok(n) => n,
        Err(_) => infer_filename_from_url(&item.url),
    };

    item.filename = filename.clone();
    item.file_path = Path::new(target_dir)
        .join(&filename)
        .to_string_lossy()
        .to_string();

    let args = build_aria2c_args(target_dir, &filename, &item.url, config);

    let pb = if let Some(m) = mp {
        let pb = m.add(ProgressBar::new_spinner());
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} {msg}")?
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
        );
        pb.set_message(format!("Downloading {}", filename));
        Some(pb)
    } else {
        None
    };

    let mut cmd = Command::new("aria2c");
    cmd.args(&args);

    #[cfg(unix)]
    {
        cmd.process_group(0);
    }

    if config.quiet || mp.is_some() {
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::piped());
    } else {
        cmd.stdout(Stdio::inherit());
        cmd.stderr(Stdio::inherit());
    }

    let mut child = cmd.spawn().context("Failed to spawn aria2c")?;

    let status = tokio::select! {
        res = child.wait() => res?,
        _ = cancel_token.cancelled() => {
            #[cfg(unix)]
            unsafe {
                if let Some(id) = child.id() {
                    // Kill process group (-id)
                    libc::kill(-(id as i32), libc::SIGTERM);
                }
            }

            #[cfg(not(unix))]
            let _ = child.start_kill();

            let _ = child.wait().await;

            if let Some(bar) = pb {
                bar.finish_with_message(format!("⚠ Cancelled {}", filename));
            }
            return Err(anyhow::anyhow!("cancelled"));
        }
    };

    if let Some(bar) = pb {
        if status.success() {
            bar.finish_with_message(format!("✔ Downloaded {}", filename));
        } else {
            bar.finish_with_message(format!("✘ Failed {}", filename));
        }
    }

    if !status.success() {
        match status.code() {
            Some(3) => anyhow::bail!("file not found or access denied"),
            Some(9) => anyhow::bail!("not enough disk space available"),
            Some(28) => anyhow::bail!("network timeout or connection refused"),
            Some(c) => anyhow::bail!("aria2c failed with exit code {}", c),
            None => anyhow::bail!("aria2c terminated by signal"),
        }
    }

    Ok(())
}
