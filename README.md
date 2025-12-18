# dlrs

> Async aria2c wrapper with sane defaults

A lightweight Rust CLI that wraps `aria2c` with optimized settings for everyday downloading. Handles filename resolution, parallel batch downloads, and graceful interrupts out of the box.

**Linux & macOS only**

---

## Features

- **Smart filenames** — Resolves names via `Content-Disposition` headers before download
- **Batch downloads** — Parallel processing with configurable concurrency
- **Optimized defaults** — 16 connections, 32 splits, falloc allocation
- **Auto-retry** — Transient failures retry with exponential backoff
- **Clean UI** — Progress bars for batches, detailed output for single files
- **Graceful shutdown** — Proper SIGTERM/SIGKILL handling on Ctrl+C

---

## Requirements

```bash
# Arch
sudo pacman -S aria2

# Debian/Ubuntu
sudo apt install aria2

# macOS
brew install aria2
```

---

## Install

```bash
git clone https://github.com/Evren-os/dlrs.git
cd dlrs
cargo build --release
sudo cp target/release/dlrs /usr/local/bin/
```

---

## Usage

```bash
# Single file
dlrs https://example.com/file.zip

# Multiple files (4 concurrent)
dlrs --parallel 4 https://example.com/a.zip https://example.com/b.zip

# Custom destination
dlrs -d ~/Downloads https://example.com/file.zip

# With speed limit
dlrs --max-speed 2M https://example.com/large.iso
```

---

## Options

| Flag | Description | Default |
|:-----|:------------|:-------:|
| `-d, --destination` | Target directory | `.` |
| `--parallel` | Concurrent downloads | `2` |
| `--max-speed` | Bandwidth limit (e.g. `1M`) | — |
| `--timeout` | Download timeout (sec) | `60` |
| `--connect-timeout` | Connection timeout (sec) | `30` |
| `--max-tries` | Max retry attempts | `5` |
| `--retry-wait` | Delay between retries (sec) | `10` |
| `--auto-retry` | Auto-retries for transient errors | `2` |
| `--user-agent` | Custom User-Agent | — |
| `-q, --quiet` | Suppress output | `false` |

---

## License

See [LICENSE](LICENSE)
