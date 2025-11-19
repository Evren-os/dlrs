use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "dlfast",
    version = "1.0",
    about = "High-performance basic download tool powered by aria2c",
    long_about = "dlfast is a basic wrapper around aria2c that provides optimized defaults and a modern CLI experience."
)]
pub struct Cli {
    /// Target directory for downloads
    #[arg(short = 'd', long)]
    pub destination: Option<String>,

    /// Maximum download speed (e.g., 1M, 500K)
    #[arg(long = "max-speed")]
    pub max_speed: Option<String>,

    /// Download timeout in seconds
    #[arg(long, default_value_t = 60)]
    pub timeout: u64,

    /// Connection timeout in seconds
    #[arg(long = "connect-timeout", default_value_t = 30)]
    pub connect_timeout: u64,

    /// Maximum retry attempts
    #[arg(long = "max-tries", default_value_t = 5)]
    pub max_tries: u32,

    /// Wait time between retries in seconds
    #[arg(long = "retry-wait", default_value_t = 10)]
    pub retry_wait: u64,

    /// Custom User-Agent string
    #[arg(long = "user-agent")]
    pub user_agent: Option<String>,

    /// Number of parallel downloads (batch mode)
    #[arg(long = "parallel", default_value_t = 2)]
    pub parallel_downloads: usize,

    /// Suppress progress display
    #[arg(long, short = 'q')]
    pub quiet: bool,

    /// URLs to download
    #[arg(required = true)]
    pub urls: Vec<String>,
}
