# dlrs

A lightweight, asynchronous wrapper for `aria2c` written in Rust. 

This is a port of a personal legacy Go tool, designed to simplify the usage of `aria2c` by applying a specific set of "sane defaults" for general file downloading. It is built for personal utility and covers 99% of my standard downloading needs, removing the necessity to manually configure connection splitting, user agents, or filename resolution for every download.

**Note:** This tool is designed primarily for **Linux and macOS** (Unix-like) systems.

## Features

*   **Smart Filename Detection**: Resolves filenames via HTTP `HEAD` requests and `Content-Disposition` headers before `aria2c` starts, preventing generic output names.
*   **Batch Processing**: Handles multiple URLs in parallel with a configurable concurrency limit.
*   **Opinionated Defaults**: Automatically configures `aria2c` with optimized settings (8 connections per server, 32 splits, fallocation) for stable and fast downloads.
*   **Clean UI**: Replaces verbose logs with simple progress spinners for batch operations, while retaining detailed output for single files.
*   **Resilient**: Handles interruptions (Ctrl+C) gracefully by ensuring child processes are terminated correctly.

## Prerequisites

*   **aria2c**: Must be installed and available in your system `PATH`.
    *   Linux (Arch): `sudo pacman -S aria2`
    *   Linux (Debian/Ubuntu): `sudo apt install aria2`
    *   macOS: `brew install aria2`

## Installation

**This project is distributed as source-only.** It is not available on crates.io or the AUR.

```bash
git clone https://github.com/Evren-os/dlrs.git
cd dlrs
cargo build --release
# Optional: Install to your path
sudo cp target/release/dlrs /usr/local/bin/dlrs
```

## Usage

**Single File Download**
Downloads a single file with full `aria2c` terminal output.

```bash
dlrs https://example.com/file.zip
```

**Batch Download**
Downloads multiple files in parallel (default: 2) with a minimized UI.

```bash
dlrs --parallel 4 https://example.com/a.zip https://example.com/b.zip https://example.com/c.zip
```

**Custom Directory**

```bash
dlrs -d ~/Downloads https://example.com/image.png
```

### Options

| Flag | Description | Default |
| :--- | :--- | :--- |
| `-d, --destination` | Target directory for downloads | Current Dir |
| `--parallel` | Number of concurrent downloads | `2` |
| `--max-speed` | Bandwidth limit (e.g., `1M`, `500K`) | Unlimited |
| `--timeout` | Download timeout in seconds | `60` |
| `-q, --quiet` | Suppress all output | `false` |

## License

See [LICENSE](LICENSE) file.