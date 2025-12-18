mod cli;
mod engine;
mod error;
mod utils;

use crate::cli::Cli;
use crate::engine::download_file;
use crate::error::DlrsError;
use crate::utils::{setup_destination, validate_url};
use clap::Parser;
use colored::Colorize;
use futures::stream::{self, StreamExt};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio_util::sync::CancellationToken;

fn check_aria2c() -> Result<(), DlrsError> {
    Command::new("aria2c")
        .arg("--version")
        .output()
        .map_err(|_| DlrsError::Aria2cNotFound)?;
    Ok(())
}

fn log_info(msg: &str) {
    println!("{} {}", "[INFO]".cyan(), msg);
}

fn log_success(msg: &str) {
    println!("{} {}", "[SUCCESS]".green(), msg);
}

fn log_warning(msg: &str) {
    println!("{} {}", "[WARNING]".yellow(), msg);
}

fn log_error(msg: &str) {
    eprintln!("{} {}", "[ERROR]".red(), msg);
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    if let Err(e) = check_aria2c() {
        log_error(&e.to_string());
        std::process::exit(1);
    }

    let cancel_token = tokio_util::sync::CancellationToken::new();
    let cancel_token_clone = cancel_token.clone();

    tokio::spawn(async move {
        if signal::ctrl_c().await.is_ok() {
            eprintln!(
                "\n{} Received interrupt, terminating downloads...",
                "[WARNING]".yellow()
            );
            cancel_token_clone.cancel();
        }
    });

    match run_downloads(&cli, cancel_token).await {
        Ok(()) => {
            if !cli.quiet {
                let msg = if cli.urls.len() == 1 {
                    "Download completed successfully!"
                } else {
                    "All downloads completed successfully!"
                };
                log_success(msg);
            }
        }
        Err(DlrsError::Cancelled) => {
            log_warning("Downloads cancelled.");
            std::process::exit(130);
        }
        Err(e) => {
            log_error(&e.to_string());
            std::process::exit(1);
        }
    }
}

async fn run_downloads(cli: &Cli, cancel_token: CancellationToken) -> Result<(), DlrsError> {
    for url in &cli.urls {
        validate_url(url)?;
    }

    let target_dir = setup_destination(cli.destination.as_ref())?;
    let target_dir_str = target_dir.to_string_lossy().to_string();

    if !cli.quiet {
        let msg = if cli.urls.len() == 1 {
            "Starting download...".to_string()
        } else {
            format!("Starting batch download of {} files...", cli.urls.len())
        };
        log_info(&msg);
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(cli.connect_timeout))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|e| DlrsError::Other(e.to_string()))?;

    let client = Arc::new(client);
    let mp = (!cli.quiet).then(MultiProgress::new).map(Arc::new);
    let cli = Arc::new(cli.clone());
    let target_dir_str = Arc::new(target_dir_str);

    let main_pb = mp.as_deref().and_then(|m| {
        (cli.urls.len() > 1).then(|| {
            let pb = m.add(ProgressBar::new(cli.urls.len() as u64));
            pb.set_style(
                ProgressStyle::with_template("{bar:40.green/white} {pos}/{len} Files")
                    .expect("Invalid template")
                    .progress_chars("##-"),
            );
            pb.enable_steady_tick(Duration::from_millis(100));
            pb
        })
    });

    let mut stream = stream::iter(cli.urls.clone())
        .map(|url| {
            let client = client.clone();
            let cli = cli.clone();
            let target_dir_str = target_dir_str.clone();
            let mp = mp.clone();
            let cancel_token = cancel_token.clone();
            let main_pb = main_pb.clone();

            async move {
                let res = download_file(
                    &client,
                    &url,
                    &target_dir_str,
                    &cli,
                    mp.as_deref(),
                    cancel_token,
                )
                .await;

                if let Some(pb) = main_pb {
                    pb.inc(1);
                }

                res.map_err(|e| DlrsError::Other(format!("{url}: {e}")))
            }
        })
        .buffer_unordered(cli.parallel_downloads);

    let mut errors = Vec::new();

    while let Some(res) = stream.next().await {
        match res {
            Err(DlrsError::Cancelled) => return Err(DlrsError::Cancelled),
            Err(DlrsError::Other(ref s)) if s.contains("cancelled") => {
                return Err(DlrsError::Cancelled);
            }
            Err(e) => errors.push(e),
            Ok(()) => {}
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        let messages: Vec<_> = errors.iter().map(|e| e.to_string()).collect();
        Err(DlrsError::Other(format!(
            "Some downloads failed:\n  {}",
            messages.join("\n  ")
        )))
    }
}
