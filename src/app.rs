use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::Color,
    widgets::ListState,
    DefaultTerminal, Frame,
};
use tokio::sync::mpsc;
use tracing::{debug, error};

use crate::config::Config;
use crate::error::Result;
use crate::library::Library;
use crate::nyaa::{NyaaCategory, NyaaClient, NyaaFilter, NyaaResult};
use crate::player::MpvPlayer;
use crate::torrent::{AnyTorrentClient, QBittorrentClient, TransmissionClient, TorrentStatus};
use crate::ui::{
    render_downloads_view, render_episodes_view, render_library_view, render_search_view, widgets,
};

const VIDEO_EXTENSIONS: &[&str] = &["mkv", "mp4", "avi", "webm", "m4v", "mov", "wmv"];

fn find_video_in_dir(dir: &Path) -> Result<PathBuf> {
    // First, try to find video files directly in the directory
    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut videos: Vec<_> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.extension()
                        .and_then(|e| e.to_str())
                        .map(|e| VIDEO_EXTENSIONS.contains(&e.to_lowercase().as_str()))
                        .unwrap_or(false)
            })
            .collect();

        videos.sort();

        if let Some(video) = videos.into_iter().next() {
            return Ok(video);
        }
    }

    // If no video found, return error
    Err(crate::error::Error::Io(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("No video file found in {:?}", dir),
    )))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Library,
    Episodes,
    Search,
    Downloads,
}

/// Messages sent from async tasks back to the main app
pub enum AppMessage {
    SearchResults(Vec<NyaaResult>),
    SearchError(String),
    TorrentAdded(String),
    TorrentError(String),
    TorrentList(Vec<TorrentStatus>),
}

pub struct App {
    pub config: Config,
    pub library: Library,
    pub running: bool,
    pub view: View,
    pub accent: Color,

    // Library view state
    pub library_state: ListState,
    pub episodes_state: ListState,
    pub selected_show_idx: Option<usize>,

    // Search view state
    pub search_query: String,
    pub search_results: Vec<NyaaResult>,
    pub search_state: ListState,
    pub search_loading: bool,
    pub search_category: NyaaCategory,
    pub search_filter: NyaaFilter,

    // Downloads view state
    pub torrents: Vec<TorrentStatus>,
    pub downloads_state: ListState,

    // Async communication
    pub msg_tx: mpsc::UnboundedSender<AppMessage>,
    pub msg_rx: mpsc::UnboundedReceiver<AppMessage>,

    // Clients
    pub nyaa_client: Arc<NyaaClient>,
    pub torrent_client: Option<Arc<AnyTorrentClient>>,
}

impl App {
    pub fn new(config: Config, library: Library) -> Self {
        let accent = widgets::parse_accent_color(&config.ui.accent_color);

        let mut library_state = ListState::default();
        if !library.shows.is_empty() {
            library_state.select(Some(0));
        }

        let (msg_tx, msg_rx) = mpsc::unbounded_channel();

        // Initialize torrent client based on config
        let torrent_client = create_torrent_client(&config);

        Self {
            config,
            library,
            running: true,
            view: View::Library,
            accent,

            library_state,
            episodes_state: ListState::default(),
            selected_show_idx: None,

            search_query: String::new(),
            search_results: Vec::new(),
            search_state: ListState::default(),
            search_loading: false,
            search_category: NyaaCategory::AnimeEnglish, // Default to English subs
            search_filter: NyaaFilter::NoFilter,

            torrents: Vec::new(),
            downloads_state: ListState::default(),

            msg_tx,
            msg_rx,

            nyaa_client: Arc::new(NyaaClient::new()),
            torrent_client: torrent_client.map(Arc::new),
        }
    }

    pub async fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        // Start periodic torrent list refresh
        self.refresh_torrent_list();

        while self.running {
            terminal.draw(|frame| self.render(frame))?;
            self.handle_events().await?;
            self.process_messages();
        }
        Ok(())
    }

    fn process_messages(&mut self) {
        while let Ok(msg) = self.msg_rx.try_recv() {
            match msg {
                AppMessage::SearchResults(results) => {
                    self.search_loading = false;
                    self.search_results = results;
                    if !self.search_results.is_empty() {
                        self.search_state.select(Some(0));
                    }
                }
                AppMessage::SearchError(err) => {
                    self.search_loading = false;
                    error!(error = %err, "Search failed");
                }
                AppMessage::TorrentAdded(hash) => {
                    debug!(hash = %hash, "Torrent added");
                    self.refresh_torrent_list();
                }
                AppMessage::TorrentError(err) => {
                    error!(error = %err, "Torrent operation failed");
                }
                AppMessage::TorrentList(torrents) => {
                    self.torrents = torrents;
                    if !self.torrents.is_empty() && self.downloads_state.selected().is_none() {
                        self.downloads_state.select(Some(0));
                    }
                }
            }
        }
    }

    fn render(&mut self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .split(frame.area());

        let main_area = chunks[0];
        let help_area = chunks[1];

        match self.view {
            View::Library => {
                render_library_view(
                    frame,
                    main_area,
                    &self.library.shows,
                    &mut self.library_state,
                    self.accent,
                );

                let help = widgets::help_bar(&[
                    ("j/k", "navigate"),
                    ("Enter/l", "select"),
                    ("/", "search"),
                    ("d", "downloads"),
                    ("r", "refresh"),
                    ("q", "quit"),
                ]);
                frame.render_widget(help, help_area);
            }
            View::Episodes => {
                if let Some(idx) = self.selected_show_idx {
                    if let Some(show) = self.library.shows.get(idx) {
                        render_episodes_view(
                            frame,
                            main_area,
                            show,
                            &mut self.episodes_state,
                            self.accent,
                        );
                    }
                }

                let help = widgets::help_bar(&[
                    ("j/k", "navigate"),
                    ("Enter", "play"),
                    ("Space", "toggle watched"),
                    ("h/Esc", "back"),
                    ("q", "quit"),
                ]);
                frame.render_widget(help, help_area);
            }
            View::Search => {
                render_search_view(
                    frame,
                    main_area,
                    &self.search_query,
                    &self.search_results,
                    &mut self.search_state,
                    self.search_loading,
                    self.search_category,
                    self.search_filter,
                    self.accent,
                );

                let help = widgets::help_bar(&[
                    ("Enter", "search/download"),
                    ("C-c", "category"),
                    ("C-f", "filter"),
                    ("Esc", "back"),
                ]);
                frame.render_widget(help, help_area);
            }
            View::Downloads => {
                render_downloads_view(
                    frame,
                    main_area,
                    &self.torrents,
                    &mut self.downloads_state,
                    self.accent,
                );

                let help = widgets::help_bar(&[
                    ("Enter", "play"),
                    ("j/k", "navigate"),
                    ("p", "pause/resume"),
                    ("x", "remove"),
                    ("r", "refresh"),
                    ("Esc", "back"),
                ]);
                frame.render_widget(help, help_area);
            }
        }
    }

    async fn handle_events(&mut self) -> Result<()> {
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                // Global quit with Ctrl+C
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c')
                {
                    self.running = false;
                    return Ok(());
                }

                match self.view {
                    View::Library => self.handle_library_input(key.code)?,
                    View::Episodes => self.handle_episodes_input(key.code)?,
                    View::Search => self.handle_search_input(key)?,
                    View::Downloads => self.handle_downloads_input(key.code).await?,
                }
            }
        }
        Ok(())
    }

    fn handle_library_input(&mut self, key: KeyCode) -> Result<()> {
        match key {
            KeyCode::Char('q') => {
                self.running = false;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection_down(&View::Library);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection_up(&View::Library);
            }
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
                self.enter_show();
            }
            KeyCode::Char('r') => {
                self.refresh_library()?;
            }
            KeyCode::Char('/') => {
                self.view = View::Search;
                self.search_query.clear();
                self.search_results.clear();
            }
            KeyCode::Char('d') => {
                self.view = View::Downloads;
                self.refresh_torrent_list();
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_episodes_input(&mut self, key: KeyCode) -> Result<()> {
        match key {
            KeyCode::Char('q') => {
                self.running = false;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection_down(&View::Episodes);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection_up(&View::Episodes);
            }
            KeyCode::Esc | KeyCode::Char('h') | KeyCode::Left => {
                self.view = View::Library;
                self.selected_show_idx = None;
            }
            KeyCode::Enter => {
                self.play_selected_episode()?;
            }
            KeyCode::Char(' ') => {
                self.toggle_watched();
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_search_input(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.view = View::Library;
            }
            KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.running = false;
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Cycle category
                self.search_category = self.search_category.next();
            }
            KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Cycle filter
                self.search_filter = self.search_filter.next();
            }
            KeyCode::Char(c) => {
                self.search_query.push(c);
            }
            KeyCode::Backspace => {
                self.search_query.pop();
            }
            KeyCode::Enter => {
                if self.search_results.is_empty() || self.search_state.selected().is_none() {
                    // Perform search
                    self.perform_search();
                } else {
                    // Download selected torrent
                    self.download_selected_torrent();
                }
            }
            KeyCode::Tab | KeyCode::Down => {
                if !self.search_results.is_empty() {
                    self.move_selection_down(&View::Search);
                }
            }
            KeyCode::Up => {
                self.move_selection_up(&View::Search);
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_downloads_input(&mut self, key: KeyCode) -> Result<()> {
        match key {
            KeyCode::Char('q') => {
                self.running = false;
            }
            KeyCode::Esc => {
                self.view = View::Library;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection_down(&View::Downloads);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection_up(&View::Downloads);
            }
            KeyCode::Char('r') => {
                self.refresh_torrent_list();
            }
            KeyCode::Char('p') => {
                self.toggle_torrent_pause().await;
            }
            KeyCode::Char('x') => {
                self.remove_selected_torrent().await;
            }
            KeyCode::Enter => {
                self.play_selected_download()?;
            }
            _ => {}
        }
        Ok(())
    }

    fn move_selection_down(&mut self, view: &View) {
        let (state, len) = match view {
            View::Library => (&mut self.library_state, self.library.shows.len()),
            View::Episodes => {
                let len = self
                    .selected_show_idx
                    .and_then(|i| self.library.shows.get(i))
                    .map(|s| s.episodes.len())
                    .unwrap_or(0);
                (&mut self.episodes_state, len)
            }
            View::Search => (&mut self.search_state, self.search_results.len()),
            View::Downloads => (&mut self.downloads_state, self.torrents.len()),
        };

        if len == 0 {
            return;
        }

        let next = match state.selected() {
            Some(i) => (i + 1).min(len - 1),
            None => 0,
        };
        state.select(Some(next));
    }

    fn move_selection_up(&mut self, view: &View) {
        let state = match view {
            View::Library => &mut self.library_state,
            View::Episodes => &mut self.episodes_state,
            View::Search => &mut self.search_state,
            View::Downloads => &mut self.downloads_state,
        };

        let next = match state.selected() {
            Some(i) => i.saturating_sub(1),
            None => 0,
        };
        state.select(Some(next));
    }

    fn enter_show(&mut self) {
        if let Some(idx) = self.library_state.selected() {
            if idx < self.library.shows.len() {
                self.selected_show_idx = Some(idx);
                self.view = View::Episodes;
                self.episodes_state = ListState::default();
                if !self.library.shows[idx].episodes.is_empty() {
                    self.episodes_state.select(Some(0));
                }
            }
        }
    }

    fn play_selected_episode(&mut self) -> Result<()> {
        let Some(show_idx) = self.selected_show_idx else {
            return Ok(());
        };
        let Some(ep_idx) = self.episodes_state.selected() else {
            return Ok(());
        };

        let show = &self.library.shows[show_idx];
        let episode = &show.episodes[ep_idx];

        let path = episode.full_path(&show.path);
        let start_pos = if episode.last_position > 0 && !episode.watched {
            Some(episode.last_position)
        } else {
            None
        };

        let show_id = show.id.clone();
        let episode_number = episode.number;

        let mut player = MpvPlayer::new(self.config.player.mpv.args.clone());
        player.play(&path, start_pos)?;
        player.wait()?;

        self.library.mark_watched(&show_id, episode_number);
        self.library.save()?;

        Ok(())
    }

    fn play_selected_download(&mut self) -> Result<()> {
        let Some(idx) = self.downloads_state.selected() else {
            return Ok(());
        };
        let Some(torrent) = self.torrents.get(idx) else {
            return Ok(());
        };

        // Only play if download is complete
        if torrent.progress < 1.0 {
            debug!("Torrent not complete, cannot play");
            return Ok(());
        }

        let content_path = std::path::Path::new(&torrent.content_path);
        
        // Find a playable video file
        let video_path = if content_path.is_file() {
            content_path.to_path_buf()
        } else if content_path.is_dir() {
            // Look for video files in the directory
            find_video_in_dir(content_path)?
        } else {
            debug!("Content path doesn't exist: {:?}", content_path);
            return Ok(());
        };

        let mut player = MpvPlayer::new(self.config.player.mpv.args.clone());
        player.play(&video_path, None)?;
        player.wait()?;

        Ok(())
    }

    fn toggle_watched(&mut self) {
        if let (Some(show_idx), Some(ep_idx)) =
            (self.selected_show_idx, self.episodes_state.selected())
        {
            if let Some(show) = self.library.shows.get(show_idx) {
                if let Some(ep) = show.episodes.get(ep_idx) {
                    let show_id = show.id.clone();
                    let ep_num = ep.number;
                    self.library.toggle_watched(&show_id, ep_num);
                    let _ = self.library.save();
                }
            }
        }
    }

    fn refresh_library(&mut self) -> Result<()> {
        let media_dirs = self.config.expanded_media_dirs();
        self.library.refresh(&media_dirs)?;
        self.library.save()?;

        if self.library.shows.is_empty() {
            self.library_state.select(None);
        } else if self.library_state.selected().is_none() {
            self.library_state.select(Some(0));
        }

        Ok(())
    }

    fn perform_search(&mut self) {
        if self.search_query.is_empty() || self.search_loading {
            return;
        }

        self.search_loading = true;
        self.search_results.clear();

        let query = self.search_query.clone();
        let category = self.search_category;
        let filter = self.search_filter;
        let client = Arc::clone(&self.nyaa_client);
        let tx = self.msg_tx.clone();

        tokio::spawn(async move {
            match client.search_with_options(&query, category, filter).await {
                Ok(results) => {
                    let _ = tx.send(AppMessage::SearchResults(results));
                }
                Err(e) => {
                    let _ = tx.send(AppMessage::SearchError(e.to_string()));
                }
            }
        });
    }

    fn download_selected_torrent(&mut self) {
        let Some(idx) = self.search_state.selected() else {
            return;
        };
        let Some(result) = self.search_results.get(idx) else {
            return;
        };
        let Some(client) = self.torrent_client.clone() else {
            error!("No torrent client configured");
            return;
        };

        let magnet = result.magnet_link.clone();
        let tx = self.msg_tx.clone();

        tokio::spawn(async move {
            match client.add_magnet(&magnet).await {
                Ok(hash) => {
                    let _ = tx.send(AppMessage::TorrentAdded(hash));
                }
                Err(e) => {
                    let _ = tx.send(AppMessage::TorrentError(e.to_string()));
                }
            }
        });
    }

    fn refresh_torrent_list(&mut self) {
        let Some(client) = self.torrent_client.clone() else {
            return;
        };

        let tx = self.msg_tx.clone();

        tokio::spawn(async move {
            match client.list_torrents().await {
                Ok(torrents) => {
                    let _ = tx.send(AppMessage::TorrentList(torrents));
                }
                Err(e) => {
                    let _ = tx.send(AppMessage::TorrentError(e.to_string()));
                }
            }
        });
    }

    async fn toggle_torrent_pause(&mut self) {
        let Some(idx) = self.downloads_state.selected() else {
            return;
        };
        let Some(torrent) = self.torrents.get(idx) else {
            return;
        };
        let Some(client) = self.torrent_client.clone() else {
            return;
        };

        let hash = torrent.hash.clone();
        let is_paused = torrent.state == crate::torrent::TorrentState::Paused;
        let tx = self.msg_tx.clone();

        tokio::spawn(async move {
            let result = if is_paused {
                client.resume(&hash).await
            } else {
                client.pause(&hash).await
            };

            if let Err(e) = result {
                let _ = tx.send(AppMessage::TorrentError(e.to_string()));
            }
        });

        // Refresh list after a short delay
        tokio::time::sleep(Duration::from_millis(200)).await;
        self.refresh_torrent_list();
    }

    async fn remove_selected_torrent(&mut self) {
        let Some(idx) = self.downloads_state.selected() else {
            return;
        };
        let Some(torrent) = self.torrents.get(idx) else {
            return;
        };
        let Some(client) = self.torrent_client.clone() else {
            return;
        };

        let hash = torrent.hash.clone();
        let tx = self.msg_tx.clone();

        tokio::spawn(async move {
            if let Err(e) = client.remove(&hash, false).await {
                let _ = tx.send(AppMessage::TorrentError(e.to_string()));
            }
        });

        tokio::time::sleep(Duration::from_millis(200)).await;
        self.refresh_torrent_list();
    }
}

fn create_torrent_client(config: &Config) -> Option<AnyTorrentClient> {
    let tc = &config.torrent;

    match tc.client.to_lowercase().as_str() {
        "transmission" => Some(AnyTorrentClient::Transmission(TransmissionClient::new(
            &tc.host,
            tc.port,
            tc.username.as_deref(),
            tc.password.as_deref(),
        ))),
        "qbittorrent" | "qbit" => Some(AnyTorrentClient::QBittorrent(QBittorrentClient::new(
            &tc.host,
            tc.port,
            tc.username.as_deref(),
            tc.password.as_deref(),
        ))),
        _ => {
            error!(client = %tc.client, "Unknown torrent client");
            None
        }
    }
}

pub fn init_terminal() -> io::Result<DefaultTerminal> {
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(io::stdout(), crossterm::terminal::EnterAlternateScreen)?;
    Ok(ratatui::init())
}

pub fn restore_terminal() -> io::Result<()> {
    ratatui::restore();
    Ok(())
}
