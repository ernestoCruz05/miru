# miru

A terminal-based anime library manager written in Rust.

*miru* (見る) means "to watch" in Japanese.

## Features

- **Library Management**: Automatically scans and organizes your local anime collection
- **Episode Tracking**: Track watched episodes and resume from where you left off
- **Nyaa.si Search**: Search and download anime torrents directly from the app
- **Torrent Integration**: Supports qBittorrent and Transmission clients
- **mpv Playback**: Play episodes with mpv, with customizable arguments
- **Episode Compression**: Compress episodes with zstd to save disk space, with transparent decompression on playback

## Installation

### From Source

```bash
git clone https://github.com/ernestoCruz05/miru.git
cd miru
cargo build --release
```

The binary will be at `target/release/miru`.

### Dependencies

- Rust 1.85+ (2024 edition)
- mpv (for playback)
- qBittorrent or Transmission (optional, for downloads)

## Configuration

Configuration is stored at `~/.config/miru/config.toml`:

```toml
[general]
media_dirs = ["~/Anime", "/mnt/media/Anime"]
compress_episodes = false  # Enable zstd compression for episodes
compression_level = 3      # 1-19, higher = smaller files but slower

[player.mpv]
args = ["--fullscreen"]

[ui]
accent_color = "#e06c75"

[torrent]
client = "qbittorrent"  # or "transmission"
host = "localhost"
port = 8080
username = "admin"
password = "password"
```

## Usage

```bash
miru
```

### Keybindings

#### Library View
- `j/k` or arrows: Navigate shows
- `Enter` or `l`: View episodes
- `/`: Search nyaa.si
- `d`: View downloads
- `r`: Refresh library
- `q`: Quit

#### Episodes View
- `j/k` or arrows: Navigate episodes
- `Enter`: Play episode
- `Space`: Toggle watched status
- `Esc` or `h`: Back to library

#### Search View
- Type to enter search query
- `Enter`: Search / Download selected
- `Tab/Down`: Navigate results
- `Ctrl+c`: Cycle category (All/English/Raw/Non-English)
- `Ctrl+f`: Cycle filter (None/Trusted/No Remakes)
- `Esc`: Back to library

#### Downloads View
- `j/k` or arrows: Navigate torrents
- `Enter`: Play completed download
- `p`: Pause/Resume torrent
- `x`: Remove torrent
- `r`: Refresh list
- `Esc`: Back to library

## License

MIT
