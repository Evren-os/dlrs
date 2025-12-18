use crate::cli::Cli;
use crate::error::DlrsError;
use crate::utils::{infer_filename_from_url, sanitize_filename};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use regex::Regex;
use reqwest::header::CONTENT_DISPOSITION;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

static CONTENT_DISPOSITION_FILENAME_STAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"filename\*\s*=\s*([^;]+)").expect("Invalid regex"));
static CONTENT_DISPOSITION_FILENAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"filename\s*=\s*([^;]+)").expect("Invalid regex"));

struct DownloadContext<'a> {
    url: &'a str,
    filename: &'a str,
    file_path: &'a str,
    target_dir: &'a str,
    config: &'a Cli,
    mp: Option<&'a MultiProgress>,
    cancel_token: CancellationToken,
    attempt: u32,
}

pub async fn detect_filename(
    client: &reqwest::Client,
    url: &str,
    user_agent: Option<&str>,
) -> Result<String, DlrsError> {
    let mut req = client.head(url);
    req = req.header("User-Agent", user_agent.unwrap_or("dlrs/1.0"));

    let resp = req
        .send()
        .await
        .map_err(|e| DlrsError::Other(e.to_string()))?;

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
    url::form_urlencoded::parse(parts[2].as_bytes())
        .map(|(k, _)| k.to_string())
        .next()
}

fn build_aria2c_args(
    target_dir: &str,
    filename: &str,
    url: &str,
    config: &Cli,
    reduced_connections: bool,
) -> Vec<String> {
    let (connections, splits) = if reduced_connections {
        (4, 8)
    } else {
        (16, 32)
    };

    let mut args = vec![
        format!("--dir={target_dir}"),
        format!("--out={filename}"),
        "--continue=true".into(),
        format!("--max-connection-per-server={connections}"),
        format!("--split={splits}"),
        "--min-split-size=1M".into(),
        "--file-allocation=falloc".into(),
        format!("--max-tries={}", config.max_tries),
        format!("--retry-wait={}", config.retry_wait),
        format!("--connect-timeout={}", config.connect_timeout),
        format!("--timeout={}", config.timeout),
        "--max-file-not-found=3".into(),
        "--summary-interval=1".into(),
        "--console-log-level=warn".into(),
        "--auto-file-renaming=false".into(),
        "--allow-overwrite=true".into(),
        "--conditional-get=true".into(),
        "--disk-cache=128M".into(),
        "--async-dns=true".into(),
        "--http-accept-gzip=true".into(),
        "--remote-time=true".into(),
        "--human-readable=false".into(),
    ];

    if let Some(speed) = &config.max_speed {
        args.push(format!("--max-download-limit={speed}"));
    }
    if let Some(ua) = &config.user_agent {
        args.push(format!("--user-agent={ua}"));
    }

    args.push(url.to_string());
    args
}

pub async fn download_file(
    client: &reqwest::Client,
    url: &str,
    target_dir: &str,
    config: &Cli,
    mp: Option<&MultiProgress>,
    cancel_token: CancellationToken,
) -> Result<(), DlrsError> {
    let filename = detect_filename(client, url, config.user_agent.as_deref())
        .await
        .unwrap_or_else(|_| infer_filename_from_url(url));

    let file_path = Path::new(target_dir)
        .join(&filename)
        .to_string_lossy()
        .to_string();

    let mut ctx = DownloadContext {
        url,
        filename: &filename,
        file_path: &file_path,
        target_dir,
        config,
        mp,
        cancel_token,
        attempt: 0,
    };

    let max_retries = config.auto_retry;

    loop {
        ctx.attempt += 1;
        let reduced_connections = ctx.attempt > 1;

        match execute_download(&mut ctx, reduced_connections).await {
            Ok(()) => return Ok(()),
            Err(DlrsError::Cancelled) => return Err(DlrsError::Cancelled),
            Err(e) if e.is_transient() && ctx.attempt <= max_retries => {
                let delay = Duration::from_secs(2u64.pow(ctx.attempt));
                tokio::time::sleep(delay).await;
                continue;
            }
            Err(e) => return Err(e),
        }
    }
}

async fn execute_download(
    ctx: &mut DownloadContext<'_>,
    reduced_connections: bool,
) -> Result<(), DlrsError> {
    let pb = ctx.mp.map(|m| {
        let pb = m.add(ProgressBar::new(0));
        pb.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} [{elapsed_precise:.yellow}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} {binary_bytes_per_sec:.magenta} (ETA: {eta:.blue}) {msg}",
            )
            .expect("Invalid template")
            .progress_chars("=>-"),
        );
        pb.set_message(ctx.filename.to_string());
        pb.enable_steady_tick(Duration::from_millis(100));
        pb
    });

    let expected_size = Arc::new(AtomicU64::new(0));
    let result = spawn_and_monitor(ctx, &pb, expected_size.clone(), reduced_connections).await;

    let final_size = expected_size.load(Ordering::Relaxed);
    let file_path = Path::new(ctx.file_path);

    if matches!(
        &result,
        Err(DlrsError::DownloadFailed {
            exit_code: Some(1 | 22),
            ..
        })
    ) && verify_download(file_path, final_size)
    {
        if let Some(bar) = pb {
            bar.finish_and_clear();
        }
        return Ok(());
    }

    if let Some(bar) = pb {
        match &result {
            Ok(()) => bar.finish_and_clear(),
            Err(_) => bar.finish_with_message(format!("✘ Failed {}", ctx.filename)),
        }
    }

    result
}

async fn spawn_and_monitor(
    ctx: &DownloadContext<'_>,
    pb: &Option<ProgressBar>,
    expected_size: Arc<AtomicU64>,
    reduced_connections: bool,
) -> Result<(), DlrsError> {
    let args = build_aria2c_args(
        ctx.target_dir,
        ctx.filename,
        ctx.url,
        ctx.config,
        reduced_connections,
    );

    let mut cmd = Command::new("aria2c");
    cmd.args(&args).stdout(Stdio::piped()).stderr(Stdio::null());

    #[cfg(unix)]
    {
        cmd.process_group(0);
    }

    let mut child = cmd.spawn().map_err(|_| DlrsError::Aria2cNotFound)?;
    let Some(stdout) = child.stdout.take() else {
        return Err(DlrsError::Other("failed to capture aria2c stdout".into()));
    };
    let mut reader = BufReader::new(stdout).lines();

    let mut stdout_closed = false;

    loop {
        tokio::select! {
            status = child.wait() => {
                // Process exited - handle status immediately
                return match status {
                    Ok(s) if s.success() => Ok(()),
                    Ok(s) => Err(s.code()
                        .map(DlrsError::download_failed)
                        .unwrap_or(DlrsError::Other("aria2c terminated by signal".into()))),
                    Err(e) => Err(DlrsError::Io(e)),
                };
            }
            res = reader.next_line(), if !stdout_closed => {
                match res {
                    Ok(Some(line)) => {
                        if let (Some((down, total)), Some(bar)) =
                            (crate::utils::parse_aria2_progress(&line), pb)
                        {
                            expected_size.store(total, Ordering::Relaxed);
                            bar.set_length(total);
                            bar.set_position(down);
                        }
                    }
                    Ok(None) => {
                        stdout_closed = true;
                    }
                    Err(_) => {
                        stdout_closed = true;
                    }
                }
            }
            _ = ctx.cancel_token.cancelled() => {
                kill_child(&mut child).await;
                return Err(DlrsError::Cancelled);
            }
        }
    }
}

#[cfg(unix)]
async fn kill_child(child: &mut tokio::process::Child) {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;

    let Some(id) = child.id() else {
        let _ = child.kill().await;
        return;
    };

    let pid = Pid::from_raw(id as i32);
    let _ = killpg(pid, Signal::SIGTERM);

    tokio::select! {
        _ = child.wait() => {}
        _ = tokio::time::sleep(Duration::from_millis(500)) => {
            let _ = killpg(pid, Signal::SIGKILL);
            let _ = child.wait().await;
        }
    }
}

#[cfg(not(unix))]
async fn kill_child(child: &mut tokio::process::Child) {
    let _ = child.kill().await;
    let _ = child.wait().await;
}

fn verify_download(path: &Path, expected_size: u64) -> bool {
    if expected_size == 0 {
        return false;
    }
    std::fs::metadata(path)
        .map(|meta| meta.len() >= expected_size)
        .unwrap_or(false)
}
