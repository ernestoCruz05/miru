# 見る miru

> A sleek terminal-based anime library manager written in Rust

https://github.com/user-attachments/assets/c5ff0012-32e4-416c-bff6-95da86b750fb

## Features

| Feature | Description |
|---------|-------------|
| **Library Management** | Scans and organizes your local anime collection automatically |
| **Smart Playback** | mpv or VLC integration with resume support and progress tracking |
| **Nyaa.si Search** | Search and download torrents directly from the TUI |
| **Auto-Download** | Track series and auto-download new episodes with season-aware filtering |
| **MAL Sync** | Import "Currently Watching" anime from MyAnimeList |
| **Cover Art** | Display anime artwork in the terminal via MAL metadata |
| **Compression** | Zstd compression to save disk space on completed shows |
| **Archiving** | Archive completed shows (ghost or compressed mode) |
| **Discord RPC** | Show your current watching activity on Discord |

---

## Prerequisites

Before installing miru, make sure you have:

- **Rust toolchain** -- install via [rustup](https://rustup.rs/)
- **A video player** -- [mpv](https://mpv.io/) recommended, VLC also supported
- **A torrent client** (optional) -- [qBittorrent](https://www.qbittorrent.org/) or [Transmission](https://transmissionbt.com/) for download features
- **Windows only:** Visual Studio C++ Build Tools (see [Windows install](#windows) below)

---

## Installation

### Pre-built Binaries

When available, pre-built binaries for Linux, macOS, and Windows can be found on the [GitHub Releases](https://github.com/ernestoCruz05/miru/releases) page. Download the binary for your platform and place it somewhere in your `PATH`.

### Building from Source

#### Linux / macOS

```bash
git clone https://github.com/ernestoCruz05/miru.git
cd miru

cargo build --release

# Install to PATH (run 'miru' from anywhere)
cargo install --path .
```

> [!TIP]
> After `cargo install`, you can simply type `miru` in any terminal to launch.

#### Windows

Windows builds require the **Visual Studio C++ Build Tools**:

1. Download [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
2. In the installer, select the **"Desktop development with C++"** workload
3. Complete the installation and restart your terminal

Then build the same way:

```powershell
git clone https://github.com/ernestoCruz05/miru.git
cd miru

cargo build --release

cargo install --path .
```

> [!NOTE]
> Both PowerShell and Command Prompt work fine. The `cargo install` command places the binary in `%USERPROFILE%\.cargo\bin\`, which rustup adds to your PATH automatically.

---

## Configuration

miru uses a TOML config file created automatically on first run.

| Platform | Config path | Data path |
|----------|-------------|-----------|
| Linux | `~/.config/miru/config.toml` | `~/.local/share/miru/` |
| macOS | `~/Library/Application Support/miru/config.toml` | `~/Library/Application Support/miru/` |
| Windows | `%APPDATA%\miru\config\config.toml` | `%APPDATA%\miru\data\` |

```toml
[general]
media_dirs = ["~/Anime", "/mnt/media/Anime"]
compress_episodes = false   # Enable zstd compression
compression_level = 3       # 1-19 (higher = smaller, slower)
archive_path = "~/.miru/archives"  # Where compressed archives are stored
archive_mode = "ghost"      # "ghost" (delete files) or "compressed" (.tar.zst)

[player.mpv]
args = ["--fullscreen"]

[ui]
accent_color = "#e06c75"

[torrent]
client = "qbittorrent"      # or "transmission"
host = "localhost"
port = 8080
password = "your-password"

[metadata]
mal_client_id = ""
```

### MyAnimeList Setup

Cover art, metadata, and MAL Sync all require a free MAL API key:

1. Log in to [MyAnimeList API Config](https://myanimelist.net/apiconfig)
2. Click **Create ID** and fill out the form:
   - **App Type:** "other"
   - **App Name:** "miru" (or anything you like)
   - **App Description:** "A terminal-based anime library manager"
   - **App Redirect URL:** `http://localhost` (required but unused)
   - **Homepage URL:** Your GitHub fork or `https://github.com/ernestoCruz05/miru`
   - **Commercial / Non-Commercial:** "non-commercial"
   - **Name / Company Name:** Your name
   - **Purpose of Use:** "hobbyist"
3. Copy the **Client ID** into your `config.toml`:

```toml
[metadata]
mal_client_id = "your_client_id_here"
```

#### Using MAL Sync

Once configured, you can import your "Currently Watching" list from MAL:

1. Open Miru and go to **Tracking List** (press `T`)
2. Press `S` to start MAL Sync
3. Copy the authorization URL shown and open it in your browser
4. Log in to MAL and authorize the app
5. Copy the authorization code from MAL's redirect page
6. Paste the code into Miru and press Enter

Your watching list will be imported with episode progress tracked -- already-watched episodes are automatically skipped.

> [!TIP]
> MAL Sync only imports series you haven't already added to your tracking list. Run it anytime to pick up new shows from your MAL.

---

## Player Setup

### mpv (recommended)

mpv is the default player. Install it for your platform:

| Platform | Install command |
|----------|----------------|
| Linux (Debian/Ubuntu) | `sudo apt install mpv` |
| Linux (Arch) | `sudo pacman -S mpv` |
| Linux (Fedora) | `sudo dnf install mpv` |
| macOS | `brew install mpv` |
| Windows | Download from [mpv.io](https://mpv.io/installation/) and add to PATH |

No extra configuration needed -- miru uses mpv by default.

```toml
# Optional: customize mpv arguments
[player.mpv]
args = ["--fullscreen", "--sub-auto=fuzzy"]
track_progress = true   # Save playback position on quit
```

### VLC (alternative)

To use VLC instead, set it as the default player and add a VLC profile:

```toml
[general]
player = "vlc"

[player.vlc]
args = []
track_progress = false
```

> [!NOTE]
> Progress tracking (resume from where you left off) works best with mpv's IPC socket. VLC support is more basic.

---

## Torrent Client Setup

A torrent client is only needed if you want to search and download from nyaa.si. miru communicates with your client's Web API.

### qBittorrent

1. Open qBittorrent -> **Tools** -> **Options** -> **Web UI**
2. Check **"Enable the Web User Interface"**
3. Set a port (default: `8080`) and password
4. Configure miru:

```toml
[torrent]
client = "qbittorrent"
host = "localhost"
port = 8080
username = "admin"
password = "your-password"
```

### Transmission

1. Start the Transmission daemon: `transmission-daemon`
2. Default Web API runs on port `9091`
3. Configure miru:

```toml
[torrent]
client = "transmission"
host = "localhost"
port = 9091
```

> [!TIP]
> Set `managed_daemon_command` to have miru start your torrent client automatically when needed:
> ```toml
> managed_daemon_command = "qbittorrent-nox"  # or "transmission-daemon"
> ```

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
| `A` | Archive show |
| `V` | View archived shows |
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
| `Ctrl+G` | Toggle torrent glossary |
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

<details>
<summary><b>Tracking List</b></summary>

| Key | Action |
|-----|--------|
| `j/k` or arrows | Navigate tracked series |
| `S` | Sync with MyAnimeList |
| `x` | Stop tracking series |
| `Esc` | Back |

</details>

---

## Troubleshooting

### "mpv not found" or "mpv not recognized"

mpv needs to be in your system PATH.

- **Linux:** Install via your package manager (see [Player Setup](#mpv-recommended))
- **macOS:** `brew install mpv`
- **Windows:** After downloading mpv, either add its folder to your system PATH or set the full path in config:
  ```toml
  [general]
  player = "C:\\mpv\\mpv.exe"
  ```

### "Connection refused" from torrent client

Your torrent client's Web API isn't reachable. Check that:

1. The Web UI is enabled in your client's settings
2. The port in `config.toml` matches your client's Web UI port
3. The client is actually running (or set `managed_daemon_command` to auto-start it)

### Windows: "LINK : fatal error LNK1181" during build

This means the Visual Studio C++ Build Tools aren't installed. Download them from [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) and install the **"Desktop development with C++"** workload.

### Config file location

Not sure where your config lives? Check the table in [Configuration](#configuration). On first run, miru creates a default config automatically.

### "library.toml not found"

This is normal on first launch. miru creates `library.toml` automatically when it first scans your media directories. Just make sure `media_dirs` in your config points to a valid folder.

### Playback won't resume where I left off

Progress tracking only works with mpv (it uses mpv's IPC socket to save your position). If you're using VLC, playback always starts from the beginning.

---

## License

MIT
