# 見る miru

> A sleek terminal-based anime library manager written in Rust

https://github.com/user-attachments/assets/c5ff0012-32e4-416c-bff6-95da86b750fb

## Features

| Feature | Description |
|---------|-------------|
| **Library Management** | Automatically scans and organizes your local anime collection |
| **Smart Playback** | mpv or VLC integration with resume support |
| **Nyaa.si Search** | Search and download torrents directly from the TUI |
| **Auto-Download** | Track series and auto-download new episodes with filters |
| **Cover Art** | Display anime artwork in the terminal |
| **Compression** | Zstd compression to save disk space |
| **Discord RPC** | Show your current activity on Discord |

---

## Installation

### From Source

```bash
# Clone the repository
git clone https://github.com/ernestoCruz05/miru.git
cd miru

# Build release binary
cargo build --release

# Install to PATH (run 'miru' from anywhere)
cargo install --path .
```

> [!TIP]
> After `cargo install`, you can simply type `miru` in any terminal to launch!

### Dependencies

- **Rust** 1.93
- **mpv** — for playback
- **qBittorrent** or **Transmission** — for downloads (optional)

---

## Configuration

Config file: `~/.config/miru/config.toml`

```toml
[general]
media_dirs = ["~/Anime", "/mnt/media/Anime"]
compress_episodes = false   # Enable zstd compression
compression_level = 3       # 1-19 (higher = smaller, slower)

[player.mpv]
args = ["--fullscreen"]

[ui]
accent_color = "#e06c75"

[torrent]
client = "qbittorrent"      # or "transmission"
host = "localhost"
port = 8080
password = "your-password"
managed_daemon_command = "qbittorrent-nox"  # Auto-launch daemon

[metadata]
mal_client_id = ""          # For MyAnimeList integration
```

### MyAnimeList Setup

1. Log in to [MyAnimeList API Config](https://myanimelist.net/apiconfig)
2. Click **Create ID** (App Type: "Other", Name: "Miru")
3. Copy the **Client ID** to `config.toml`

---

## Usage

```bash
miru
```

### Keybindings

<details>
<summary><b>Library View</b></summary>

| Key | Action |
|-----|--------|
| `j/k` or arrows | Navigate shows |
| `Enter` / `l` | View episodes |
| `/` | Search Nyaa.si |
| `t` | Track new series |
| `T` | View tracked series |
| `d` | View downloads |
| `r` | Refresh library |
| `m` | Fetch MAL metadata |
| `x` | Delete show |
| `?` | Help |
| `q` | Quit |

</details>

<details>
<summary><b>Episodes View</b></summary>

| Key | Action |
|-----|--------|
| `j/k` or arrows | Navigate episodes |
| `Enter` | Play episode |
| `Space` | Toggle watched |
| `x` | Delete episode |
| `Esc` / `h` | Back to library |

</details>

<details>
<summary><b>Search View</b></summary>

| Key | Action |
|-----|--------|
| *type* | Enter search query |
| `Enter` | Search / Download |
| `Tab` / Down | Navigate results |
| `Ctrl+C` | Cycle category |
| `Ctrl+F` | Cycle filter |
| `Ctrl+S` | Cycle sort |
| `/` | Filter results |
| `Esc` | Back |

</details>

<details>
<summary><b>Downloads View</b></summary>

| Key | Action |
|-----|--------|
| `j/k` or arrows | Navigate torrents |
| `Enter` | Move completed to library |
| `p` | Pause/Resume |
| `x` | Remove torrent |
| `r` | Refresh |
| `Esc` | Back |

</details>

---

## License

MIT
