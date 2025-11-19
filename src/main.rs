mod cli;
mod engine;
mod utils;

use crate::cli::Cli;
use crate::engine::{DownloadItem, download_file};
use crate::utils::{setup_destination, validate_url};
use clap::Parser;
use colored::Colorize;
use futures::stream::{self, StreamExt};
use indicatif::MultiProgress;
use std::process::Command;
use std::sync::Arc;
use tokio::signal;

fn check_aria2c() -> anyhow::Result<()> {
    match Command::new("aria2c").arg("--version").output() {
        Ok(_) => Ok(()),
        Err(_) => anyhow::bail!("aria2c not found in PATH. Please install aria2c."),
    }
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
        if let Ok(()) = signal::ctrl_c().await {
            eprintln!(
                "\n{} Received interrupt signal, cancelling downloads...",
                "[WARNING]".yellow()
            );
            cancel_token_clone.cancel();
        }
    });

    if let Err(e) = run_downloads(&cli, cancel_token).await {
        if e.to_string().contains("cancelled") {
            log_warning("Downloads cancelled.");
            std::process::exit(130);
        }
        log_error(&format!("{:?}", e));
        std::process::exit(1);
    }

    if !cli.quiet {
        if cli.urls.len() == 1 {
            log_success("Download completed successfully!");
        } else {
            log_success("All downloads completed successfully!");
        }
    }
}

async fn run_downloads(
    cli: &Cli,
    cancel_token: tokio_util::sync::CancellationToken,
) -> anyhow::Result<()> {
    for url in &cli.urls {
        validate_url(url)?;
    }

    let target_dir = setup_destination(cli.destination.as_ref())?;
    let target_dir_str = target_dir.to_string_lossy().to_string();

    if !cli.quiet {
        if cli.urls.len() == 1 {
            log_info("Starting download...");
        } else {
            log_info(&format!(
                "Starting batch download of {} files...",
                cli.urls.len()
            ));
        }
    }

    let mp = if cli.parallel_downloads > 1 && !cli.quiet {
        Some(MultiProgress::new())
    } else {
        None
    };

    let cli = Arc::new(cli.clone());
    let target_dir_str = Arc::new(target_dir_str);
    let mp = Arc::new(mp);

    let downloads = cli
        .urls
        .iter()
        .map(|u| DownloadItem {
            url: u.clone(),
            filename: String::new(),
            file_path: String::new(),
        })
        .collect::<Vec<_>>();

    let mut stream = stream::iter(downloads)
        .map(|mut item| {
            let cli = cli.clone();
            let target_dir_str = target_dir_str.clone();
            let mp = mp.clone();
            let cancel_token = cancel_token.clone();

            async move {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        Err(anyhow::anyhow!("cancelled"))
                    }
                    res = download_file(&mut item, &target_dir_str, &cli, mp.as_ref().as_ref(), cancel_token.clone()) => {
                        if let Err(e) = res {
                            Err(anyhow::anyhow!("Failed: {} - {}", item.url, e))
                        } else {
                            Ok(())
                        }
                    }
                }
            }
        })
        .buffer_unordered(cli.parallel_downloads);

    let mut errors = Vec::new();

    while let Some(res) = stream.next().await {
        if let Err(e) = res {
            if e.to_string().contains("cancelled") {
                return Err(anyhow::anyhow!("cancelled"));
            }
            errors.push(e);
        }
    }

    if !errors.is_empty() {
        return Err(anyhow::anyhow!("some downloads failed: {:?}", errors));
    }

    Ok(())
}
