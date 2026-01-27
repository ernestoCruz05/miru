use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use std::thread;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::Color,
    widgets::ListState,
    DefaultTerminal, Frame,
};
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::compression;
use crate::config::Config;
use crate::error::Result;
use crate::library::Library;
use crate::nyaa::{NyaaCategory, NyaaClient, NyaaFilter, NyaaResult, NyaaSort};
use crate::player::ExternalPlayer;
use crate::torrent::{AnyTorrentClient, QBittorrentClient, TransmissionClient, TorrentStatus};
use crate::ui::{
    render_downloads_view, render_episodes_view, render_library_view, render_search_view, widgets,
};

const VIDEO_EXTENSIONS: &[&str] = &["mkv", "mp4", "avi", "webm", "m4v", "mov", "wmv"];

/// Clean up a torrent filename to a more readable format
/// e.g., "[SubGroup] Show Name - 01 (1080p) [HASH].mkv" -> "Show Name - S01E01.mkv"
fn clean_filename(name: &str) -> String {
    let mut clean = name.to_string();
    
    // Remove common patterns
    // Remove [...] bracketed content (subgroup, hash, quality info)
    while let (Some(start), Some(end)) = (clean.find('['), clean.find(']')) {
        if start < end {
            clean = format!("{}{}", &clean[..start], &clean[end + 1..]);
        } else {
            break;
        }
    }
    
    // Remove (...) parenthetical content
    while let (Some(start), Some(end)) = (clean.find('('), clean.find(')')) {
        if start < end {
            clean = format!("{}{}", &clean[..start], &clean[end + 1..]);
        } else {
            break;
        }
    }
    
    // Clean up multiple spaces and dots
    clean = clean.replace("  ", " ").replace("..", ".").trim().to_string();
    
    // Try to extract episode number and format nicely
    // Look for patterns like "- 01", "E01", "EP01", "Episode 01"
    let episode_patterns = [
        (regex::Regex::new(r"[Ss](\d{1,2})[Ee](\d{1,3})").unwrap(), true),  // S01E01
        (regex::Regex::new(r"[Ee][Pp]?\.?\s*(\d{1,3})").unwrap(), false),   // E01, EP01, Ep 01
        (regex::Regex::new(r"\s-\s*(\d{1,3})\b").unwrap(), false),          // - 01
        (regex::Regex::new(r"#(\d{1,3})").unwrap(), false),                  // #01
    ];
    
    for (re, has_season) in &episode_patterns {
        if let Some(caps) = re.captures(&clean) {
            if *has_season {
                let season: u32 = caps.get(1).unwrap().as_str().parse().unwrap_or(1);
                let episode: u32 = caps.get(2).unwrap().as_str().parse().unwrap_or(1);
                // Get the show name (everything before the match)
                let show_name = clean[..caps.get(0).unwrap().start()].trim();
                let show_name = show_name.trim_end_matches(&['-', '.', ' '][..]);
                let ext = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("mkv");
                return format!("{} - S{:02}E{:02}.{}", show_name, season, episode, ext);
            } else {
                let episode: u32 = caps.get(1).unwrap().as_str().parse().unwrap_or(1);
                let show_name = clean[..caps.get(0).unwrap().start()].trim();
                let show_name = show_name.trim_end_matches(&['-', '.', ' '][..]);
                let ext = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("mkv");
                return format!("{} - E{:02}.{}", show_name, episode, ext);
            }
        }
    }
    
    // If no pattern matched, just return cleaned name
    clean.trim().to_string()
}

/// List subdirectories in a path (for show folders)
fn list_subdirs(path: &Path) -> Vec<String> {
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default()
}

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
    MoveDialog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveDialogStep {
    SelectMediaDir,
    SelectShow,
    EditFilename,
}

/// State for the move-to-library dialog
pub struct MoveDialogState {
    pub step: MoveDialogStep,
    pub torrent_idx: usize,
    pub media_dirs: Vec<PathBuf>,
    pub media_dir_state: ListState,
    pub selected_media_dir: Option<PathBuf>,
    pub shows_in_dir: Vec<String>,
    pub show_state: ListState,
    pub selected_show: Option<String>,
    pub new_show_name: String,
    pub creating_new: bool,
    pub filename: String,
    pub original_path: PathBuf,
}

impl Default for MoveDialogState {
    fn default() -> Self {
        Self {
            step: MoveDialogStep::SelectMediaDir,
            torrent_idx: 0,
            media_dirs: Vec::new(),
            media_dir_state: ListState::default(),
            selected_media_dir: None,
            shows_in_dir: Vec::new(),
            show_state: ListState::default(),
            selected_show: None,
            new_show_name: String::new(),
            creating_new: false,
            filename: String::new(),
            original_path: PathBuf::new(),
        }
    }
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
    pub filtered_search_results: Vec<usize>, // Indices of filtered results
    pub search_filter_input: String,
    pub is_filtering: bool,
    pub search_state: ListState,
    pub search_loading: bool,
    pub search_category: NyaaCategory,
    pub search_filter: NyaaFilter,
    pub search_sort: NyaaSort,

    // Downloads view state
    pub torrents: Vec<TorrentStatus>,
    pub downloads_state: ListState,

    // Move dialog state
    pub move_dialog: MoveDialogState,

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
            filtered_search_results: Vec::new(),
            search_filter_input: String::new(),
            is_filtering: false,
            search_state: ListState::default(),
            search_loading: false,
            search_category: NyaaCategory::AnimeEnglish, // Default to English subs
            search_filter: NyaaFilter::NoFilter,
            search_sort: NyaaSort::default(),

            torrents: Vec::new(),
            downloads_state: ListState::default(),

            move_dialog: MoveDialogState::default(),

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
                    // Initialize filtered results with everything
                    self.filtered_search_results = (0..self.search_results.len()).collect();
                    if !self.filtered_search_results.is_empty() {
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
                    ("p", "play next"),
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
                    self.search_sort,
                    self.accent,
                );

                let help = widgets::help_bar(&[
                    ("Enter", "search/download"),
                    ("C-c", "category"),
                    ("C-f", "filter"),
                    ("s", "sort"),
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
                    ("m", "move to library"),
                    ("j/k", "navigate"),
                    ("p", "pause"),
                    ("x", "remove"),
                    ("Esc", "back"),
                ]);
                frame.render_widget(help, help_area);
            }
            View::MoveDialog => {
                // Render downloads in background
                render_downloads_view(
                    frame,
                    main_area,
                    &self.torrents,
                    &mut self.downloads_state,
                    self.accent,
                );

                // Render move dialog overlay
                self.render_move_dialog(frame);

                let help_text = match self.move_dialog.step {
                    MoveDialogStep::SelectMediaDir => &[("j/k", "navigate"), ("Enter", "select"), ("Esc", "cancel")][..],
                    MoveDialogStep::SelectShow => &[("j/k", "navigate"), ("Enter", "select"), ("n", "new folder"), ("Esc", "back")][..],
                    MoveDialogStep::EditFilename => &[("Enter", "confirm"), ("Esc", "back")][..],
                };
                let help = widgets::help_bar(help_text);
                frame.render_widget(help, help_area);
            }
        }
    }

    async fn handle_events(&mut self) -> Result<()> {
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    return Ok(());
                }
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
                    View::MoveDialog => self.handle_move_dialog_input(key.code)?,
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
            KeyCode::Char('p') => {
                self.play_next_unwatched()?;
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
        if self.is_filtering {
            match key.code {
                KeyCode::Esc => {
                    self.is_filtering = false;
                    self.search_filter_input.clear();
                    self.update_filtered_results();
                }
                KeyCode::Enter => {
                    // Confirm filter (keep it active but exit edit mode? Or just stay?)
                    // For fzf style, Enter usually selects the top item. 
                    // Let's say Enter selects the currently highlighted item as usual.
                    if !self.filtered_search_results.is_empty() {
                         self.download_selected_torrent();
                    }
                }
                 KeyCode::Backspace => {
                    self.search_filter_input.pop();
                    self.update_filtered_results();
                }
                KeyCode::Char(c) => {
                    self.search_filter_input.push(c);
                    self.update_filtered_results();
                }
                KeyCode::Up => self.move_selection_up(&View::Search),
                KeyCode::Down => self.move_selection_down(&View::Search),
                _ => {}
            }
        } else {
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
                KeyCode::Char('s') => {
                    // Cycle sort
                    self.search_sort = self.search_sort.next();
                    // If results exist, re-search with new sort to respect server-side ordering
                    if !self.search_results.is_empty() {
                        self.perform_search();
                    }
                }
                // Enter filter mode with /
                KeyCode::Char('/') if !self.search_results.is_empty() => {
                    self.is_filtering = true;
                    self.search_filter_input.clear();
                    self.update_filtered_results();
                }
                KeyCode::Backspace => {
                    self.search_query.pop();
                }
                KeyCode::Enter => {
                    if self.search_results.is_empty() {
                        // Perform search
                        self.perform_search();
                    } else {
                        // Download selected torrent
                        self.download_selected_torrent();
                    }
                }
                KeyCode::Tab | KeyCode::Down | KeyCode::Char('j') => {
                    if !self.search_results.is_empty() {
                        self.move_selection_down(&View::Search);
                    }
                }
                KeyCode::Up | KeyCode::Char('k')  => {
                    self.move_selection_up(&View::Search);
                }
                KeyCode::Char(c) => {
                    self.search_query.push(c);
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn update_filtered_results(&mut self) {
        if self.search_filter_input.is_empty() {
            self.filtered_search_results = (0..self.search_results.len()).collect();
        } else {
            let filter_lower = self.search_filter_input.to_lowercase();
            self.filtered_search_results = self.search_results
                .iter()
                .enumerate()
                .filter(|(_, r)| r.title.to_lowercase().contains(&filter_lower))
                .map(|(i, _)| i)
                .collect();
        }
        // Reset selection if list changed
        if !self.filtered_search_results.is_empty() {
            self.search_state.select(Some(0));
        } else {
            self.search_state.select(None);
        }
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
            KeyCode::Char('m') => {
                self.open_move_dialog();
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
            View::Search => (&mut self.search_state, self.filtered_search_results.len()),
            View::Downloads | View::MoveDialog => (&mut self.downloads_state, self.torrents.len()),
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
            View::Search => (&mut self.search_state, self.filtered_search_results.len()),
            View::Downloads | View::MoveDialog => (&mut self.downloads_state, self.torrents.len()),
        };

        if len > 0 {
            let i = match state.selected() {
                Some(i) => {
                    if i == 0 {
                        len - 1
                    } else {
                        i - 1
                    }
                }
                None => 0,
            };
            state.select(Some(i));
        }
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

        // Check if file is compressed and decompress to temp if needed
        let (play_path, temp_path) = if compression::is_compressed(&path) {
            info!(path = %path.display(), "Decompressing episode for playback");
            let temp = compression::decompress_to_temp(&path)?;
            (temp.clone(), Some(temp))
        } else {
            (path, None)
        };

        let player_cmd = self.config.general.player.clone();
        
        // Select logic for args based on player name (fallback to mpv args if exact match not found for vlc)
        // Check if user has specific config for this player
        let args = if player_cmd == "vlc" {
             self.config.player.vlc.as_ref().map(|p| p.args.clone()).unwrap_or_else(|| vec!["--fullscreen".to_string()])
        } else {
             // Default to mpv args or empty if unknown
             self.config.player.mpv.args.clone()
        };

        let mut player = ExternalPlayer::new(player_cmd, args);
        player.play(&play_path, start_pos)?;
        player.wait()?;

        // Clean up temp file if we decompressed
        if let Some(temp) = temp_path {
            if let Some(parent) = temp.parent() {
                let _ = std::fs::remove_dir_all(parent);
            }
        }

        self.library.mark_watched(&show_id, episode_number);
        self.library.save()?;

        Ok(())
    }

    fn play_next_unwatched(&mut self) -> Result<()> {
        let Some(show_idx) = self.library_state.selected() else {
            return Ok(());
        };

        // Scope to borrow show/episode
        let (show_id, episode_number, path, start_pos) = {
            let show = &self.library.shows[show_idx];
            let Some(episode) = show.next_unwatched() else {
                return Ok(());
            };

            let path = episode.full_path(&show.path);
            let start_pos = if episode.last_position > 0 {
                Some(episode.last_position)
            } else {
                None
            };
            (show.id.clone(), episode.number, path, start_pos)
        };

        // Check if file is compressed and decompress to temp if needed
        let (play_path, temp_path) = if compression::is_compressed(&path) {
            info!(path = %path.display(), "Decompressing episode for playback");
            let temp = compression::decompress_to_temp(&path)?;
            (temp.clone(), Some(temp))
        } else {
            (path, None)
        };

        let player_cmd = self.config.general.player.clone();
        
        // Select logic for args based on player name (fallback to mpv args if exact match not found for vlc)
        // Check if user has specific config for this player
        let args = if player_cmd == "vlc" {
             self.config.player.vlc.as_ref().map(|p| p.args.clone()).unwrap_or_else(|| vec!["--fullscreen".to_string()])
        } else {
             // Default to mpv args or empty if unknown
             self.config.player.mpv.args.clone()
        };

        let mut player = ExternalPlayer::new(player_cmd, args);
        player.play(&play_path, start_pos)?;
        player.wait()?;

        // Clean up temp file if we decompressed
        if let Some(temp) = temp_path {
            if let Some(parent) = temp.parent() {
                let _ = std::fs::remove_dir_all(parent);
            }
        }

        self.library.mark_watched(&show_id, episode_number);
        self.library.save()?;

        Ok(())
    }

    fn download_selected_torrent(&mut self) {
        let Some(idx) = self.search_state.selected() else {
            return;
        };
        
        // Map filtered index to real index if filtering
        let result_idx = if !self.filtered_search_results.is_empty() {
            *self.filtered_search_results.get(idx).unwrap_or(&idx)
        } else {
             idx
        };

        let Some(result) = self.search_results.get(result_idx) else {
            return;
        };

        if let Some(client) = self.torrent_client.clone() {
            let magnet = result.magnet_link.clone();
            let tx = self.msg_tx.clone();
            
            info!(title = %result.title, "Adding torrent");

            tokio::spawn(async move {
                match client.add_magnet(&magnet).await {
                    Ok(hash) => {
                        let _ = tx.send(AppMessage::TorrentAdded(hash));
                    }
                    Err(e) => {
                        error!("Failed to add torrent: {}", e);
                    }
                }
            });
            
            // Switch to downloads view to see progress
            self.view = View::Downloads;
        }
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

        let player_cmd = self.config.general.player.clone();
         let args = if player_cmd == "vlc" {
             self.config.player.vlc.as_ref().map(|p| p.args.clone()).unwrap_or_else(|| vec!["--fullscreen".to_string()])
        } else {
             // Default to mpv args or empty if unknown
             self.config.player.mpv.args.clone()
        };

        let mut player = ExternalPlayer::new(player_cmd, args);
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
        let sort = self.search_sort;
        let client = Arc::clone(&self.nyaa_client);
        let tx = self.msg_tx.clone();

        tokio::spawn(async move {
            match client.search(&query, category, filter, sort).await {
                Ok(results) => {
                    let _ = tx.send(AppMessage::SearchResults(results));
                }
                Err(e) => {
                    let _ = tx.send(AppMessage::SearchError(e.to_string()));
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

    fn open_move_dialog(&mut self) {
        let Some(idx) = self.downloads_state.selected() else {
            return;
        };
        let Some(torrent) = self.torrents.get(idx) else {
            return;
        };

        // Only allow moving completed torrents
        if torrent.progress < 1.0 {
            debug!("Cannot move incomplete torrent");
            return;
        }

        let content_path = Path::new(&torrent.content_path);
        
        // Find the actual video file
        let video_path = if content_path.is_file() {
            content_path.to_path_buf()
        } else if content_path.is_dir() {
            match find_video_in_dir(content_path) {
                Ok(p) => p,
                Err(_) => return,
            }
        } else {
            return;
        };

        let original_filename = video_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&torrent.name);
        
        let clean_name = clean_filename(original_filename);
        let media_dirs: Vec<PathBuf> = self.config.expanded_media_dirs();

        self.move_dialog = MoveDialogState {
            step: MoveDialogStep::SelectMediaDir,
            torrent_idx: idx,
            media_dirs: media_dirs.clone(),
            media_dir_state: {
                let mut state = ListState::default();
                if !media_dirs.is_empty() {
                    state.select(Some(0));
                }
                state
            },
            selected_media_dir: None,
            shows_in_dir: Vec::new(),
            show_state: ListState::default(),
            selected_show: None,
            new_show_name: String::new(),
            creating_new: false,
            filename: clean_name,
            original_path: video_path,
        };

        self.view = View::MoveDialog;
    }

    fn handle_move_dialog_input(&mut self, key: KeyCode) -> Result<()> {
        match self.move_dialog.step {
            MoveDialogStep::SelectMediaDir => {
                match key {
                    KeyCode::Esc => {
                        self.view = View::Downloads;
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        let len = self.move_dialog.media_dirs.len();
                        if len > 0 {
                            let next = self.move_dialog.media_dir_state.selected()
                                .map(|i| (i + 1).min(len - 1))
                                .unwrap_or(0);
                            self.move_dialog.media_dir_state.select(Some(next));
                        }
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        let next = self.move_dialog.media_dir_state.selected()
                            .map(|i| i.saturating_sub(1))
                            .unwrap_or(0);
                        self.move_dialog.media_dir_state.select(Some(next));
                    }
                    KeyCode::Enter => {
                        if let Some(idx) = self.move_dialog.media_dir_state.selected() {
                            if let Some(dir) = self.move_dialog.media_dirs.get(idx).cloned() {
                                self.move_dialog.selected_media_dir = Some(dir.clone());
                                
                                // Load shows in this directory
                                let mut shows = list_subdirs(&dir);
                                shows.sort();
                                self.move_dialog.shows_in_dir = shows;
                                
                                self.move_dialog.show_state = ListState::default();
                                if !self.move_dialog.shows_in_dir.is_empty() {
                                    self.move_dialog.show_state.select(Some(0));
                                }
                                
                                self.move_dialog.step = MoveDialogStep::SelectShow;
                            }
                        }
                    }
                    _ => {}
                }
            }
            MoveDialogStep::SelectShow => {
                if self.move_dialog.creating_new {
                    // Text input mode for new folder name
                    match key {
                        KeyCode::Esc => {
                            self.move_dialog.creating_new = false;
                            self.move_dialog.new_show_name.clear();
                        }
                        KeyCode::Enter => {
                            if !self.move_dialog.new_show_name.is_empty() {
                                self.move_dialog.selected_show = Some(self.move_dialog.new_show_name.clone());
                                self.move_dialog.step = MoveDialogStep::EditFilename;
                                self.move_dialog.creating_new = false;
                            }
                        }
                        KeyCode::Backspace => {
                            self.move_dialog.new_show_name.pop();
                        }
                        KeyCode::Char(c) => {
                            self.move_dialog.new_show_name.push(c);
                        }
                        _ => {}
                    }
                } else {
                    match key {
                        KeyCode::Esc => {
                            self.move_dialog.step = MoveDialogStep::SelectMediaDir;
                        }
                        KeyCode::Char('j') | KeyCode::Down => {
                            let len = self.move_dialog.shows_in_dir.len();
                            if len > 0 {
                                let next = self.move_dialog.show_state.selected()
                                    .map(|i| (i + 1).min(len - 1))
                                    .unwrap_or(0);
                                self.move_dialog.show_state.select(Some(next));
                            }
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            let next = self.move_dialog.show_state.selected()
                                .map(|i| i.saturating_sub(1))
                                .unwrap_or(0);
                            self.move_dialog.show_state.select(Some(next));
                        }
                        KeyCode::Char('n') => {
                            self.move_dialog.creating_new = true;
                            self.move_dialog.new_show_name.clear();
                        }
                        KeyCode::Enter => {
                            if let Some(idx) = self.move_dialog.show_state.selected() {
                                if let Some(show) = self.move_dialog.shows_in_dir.get(idx).cloned() {
                                    self.move_dialog.selected_show = Some(show);
                                    self.move_dialog.step = MoveDialogStep::EditFilename;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            MoveDialogStep::EditFilename => {
                match key {
                    KeyCode::Esc => {
                        self.move_dialog.step = MoveDialogStep::SelectShow;
                    }
                    KeyCode::Enter => {
                        self.execute_move()?;
                    }
                    KeyCode::Backspace => {
                        self.move_dialog.filename.pop();
                    }
                    KeyCode::Char(c) => {
                        self.move_dialog.filename.push(c);
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn render_move_dialog(&self, frame: &mut Frame) {
        use ratatui::{
            layout::{Constraint, Flex, Layout},
            style::{Modifier, Style},
            text::{Line, Span},
            widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
        };

        let area = frame.area();
        
        // Center a dialog box
        let dialog_width = 60.min(area.width.saturating_sub(4));
        let dialog_height = 15.min(area.height.saturating_sub(4));
        
        let horizontal = Layout::horizontal([Constraint::Length(dialog_width)]).flex(Flex::Center);
        let vertical = Layout::vertical([Constraint::Length(dialog_height)]).flex(Flex::Center);
        let [dialog_area] = vertical.areas(area);
        let [dialog_area] = horizontal.areas(dialog_area);

        frame.render_widget(Clear, dialog_area);

        let title = match self.move_dialog.step {
            MoveDialogStep::SelectMediaDir => "Move to Library - Select Destination",
            MoveDialogStep::SelectShow => {
                if self.move_dialog.creating_new {
                    "Move to Library - New Folder Name"
                } else {
                    "Move to Library - Select Show Folder"
                }
            }
            MoveDialogStep::EditFilename => "Move to Library - Edit Filename",
        };

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.accent));

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        match self.move_dialog.step {
            MoveDialogStep::SelectMediaDir => {
                let items: Vec<ListItem> = self.move_dialog.media_dirs
                    .iter()
                    .map(|p| ListItem::new(p.display().to_string()))
                    .collect();

                let list = List::new(items)
                    .highlight_style(Style::default().fg(self.accent).add_modifier(Modifier::BOLD))
                    .highlight_symbol("> ");

                frame.render_stateful_widget(list, inner, &mut self.move_dialog.media_dir_state.clone());
            }
            MoveDialogStep::SelectShow => {
                if self.move_dialog.creating_new {
                    let input_text = format!("> {}_", self.move_dialog.new_show_name);
                    let para = Paragraph::new(input_text)
                        .style(Style::default().fg(self.accent));
                    frame.render_widget(para, inner);
                } else {
                    let mut items: Vec<ListItem> = self.move_dialog.shows_in_dir
                        .iter()
                        .map(|s| ListItem::new(format!("  {}/", s)))
                        .collect();
                    
                    if items.is_empty() {
                        items.push(ListItem::new(Line::from(vec![
                            Span::styled("(empty - press ", Style::default().fg(Color::DarkGray)),
                            Span::styled("n", Style::default().fg(self.accent)),
                            Span::styled(" to create new)", Style::default().fg(Color::DarkGray)),
                        ])));
                    }

                    let list = List::new(items)
                        .highlight_style(Style::default().fg(self.accent).add_modifier(Modifier::BOLD))
                        .highlight_symbol("> ");

                    frame.render_stateful_widget(list, inner, &mut self.move_dialog.show_state.clone());
                }
            }
            MoveDialogStep::EditFilename => {
                let dest_path = self.move_dialog.selected_media_dir.as_ref()
                    .map(|p| p.join(self.move_dialog.selected_show.as_ref().unwrap_or(&String::new())))
                    .unwrap_or_default();

                let lines = vec![
                    Line::from(vec![
                        Span::styled("Destination: ", Style::default().fg(Color::DarkGray)),
                        Span::raw(dest_path.display().to_string()),
                    ]),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("Filename: ", Style::default().fg(Color::DarkGray)),
                    ]),
                    Line::from(vec![
                        Span::styled(format!("{}_", self.move_dialog.filename), Style::default().fg(self.accent)),
                    ]),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("Press Enter to confirm, Esc to go back", Style::default().fg(Color::DarkGray)),
                    ]),
                ];

                let para = Paragraph::new(lines);
                frame.render_widget(para, inner);
            }
        }
    }

    fn execute_move(&mut self) -> Result<()> {
        let Some(media_dir) = &self.move_dialog.selected_media_dir else {
            return Ok(());
        };
        let Some(show_name) = &self.move_dialog.selected_show else {
            return Ok(());
        };

        let dest_dir = media_dir.join(show_name);
        
        // Create directory if it doesn't exist
        if !dest_dir.exists() {
            std::fs::create_dir_all(&dest_dir)?;
        }

        let dest_path = dest_dir.join(&self.move_dialog.filename);
        let source_path = &self.move_dialog.original_path;

        debug!(
            source = %source_path.display(),
            dest = %dest_path.display(),
            "Moving file"
        );

        // Try rename first (same filesystem), fall back to copy+delete
        if std::fs::rename(source_path, &dest_path).is_err() {
            std::fs::copy(source_path, &dest_path)?;
            std::fs::remove_file(source_path)?;
        }

        // Compress if enabled
        if self.config.general.compress_episodes {
            info!(path = %dest_path.display(), "Compressing episode");
            compression::compress_file(&dest_path, self.config.general.compression_level)?;
        }

        // Refresh library to pick up the new file
        self.refresh_library()?;

        // Go back to downloads view
        self.view = View::Downloads;

        Ok(())
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

/// ASCII art for "miru" - each letter as a frame
const MIRU_FRAMES: [&str; 4] = [
    r#"
                     
  ███╗   ███╗        
  ████╗ ████║        
  ██╔████╔██║        
  ██║╚██╔╝██║        
  ██║ ╚═╝ ██║        
  ╚═╝     ╚═╝        
                     
"#,
    r#"
                          
  ███╗   ███╗ ██╗         
  ████╗ ████║ ██║         
  ██╔████╔██║ ██║         
  ██║╚██╔╝██║ ██║         
  ██║ ╚═╝ ██║ ██║         
  ╚═╝     ╚═╝ ╚═╝         
                          
"#,
    r#"
                               
  ███╗   ███╗ ██╗ ██████╗      
  ████╗ ████║ ██║ ██╔══██╗     
  ██╔████╔██║ ██║ ██████╔╝     
  ██║╚██╔╝██║ ██║ ██╔══██╗     
  ██║ ╚═╝ ██║ ██║ ██║  ██║     
  ╚═╝     ╚═╝ ╚═╝ ╚═════╝      
                               
"#,
    r#"
                                     
  ███╗   ███╗ ██╗ ██████╗  ██╗   ██╗ 
  ████╗ ████║ ██║ ██╔══██╗ ██║   ██║ 
  ██╔████╔██║ ██║ ██████╔╝ ██║   ██║ 
  ██║╚██╔╝██║ ██║ ██╔══██╗ ██║   ██║ 
  ██║ ╚═╝ ██║ ██║ ██║  ██║ ╚██████╔╝ 
  ╚═╝     ╚═╝ ╚═╝ ╚═════╝   ╚═════╝  
                                     
"#,
];

const MIRU_TAGLINE: &str = "見る - to watch";

/// Play the splash screen animation
pub fn play_splash(terminal: &mut DefaultTerminal, accent: Color) -> io::Result<()> {
    use ratatui::{
        layout::{Alignment, Rect},
        style::Style,
        text::{Line, Text},
        widgets::Paragraph,
    };

    // Type out each letter
    for frame in &MIRU_FRAMES {
        terminal.draw(|f| {
            let area = f.area();
            let text = Text::styled(*frame, Style::default().fg(accent));
            
            // Center vertically
            let lines = frame.lines().count() as u16;
            let y_offset = area.height.saturating_sub(lines) / 2;
            
            let centered_area = Rect {
                x: 0,
                y: y_offset,
                width: area.width,
                height: lines + 2,
            };
            
            let para = Paragraph::new(text).alignment(Alignment::Center);
            f.render_widget(para, centered_area);
        })?;
        
        thread::sleep(Duration::from_millis(150));
    }

    // Show tagline
    terminal.draw(|f| {
        let area = f.area();
        let frame_text = MIRU_FRAMES[3];
        let lines = frame_text.lines().count() as u16;
        let y_offset = area.height.saturating_sub(lines + 2) / 2;
        
        let logo_area = Rect {
            x: 0,
            y: y_offset,
            width: area.width,
            height: lines,
        };
        
        let tagline_area = Rect {
            x: 0,
            y: y_offset + lines,
            width: area.width,
            height: 2,
        };
        
        let logo = Paragraph::new(Text::styled(frame_text, Style::default().fg(accent)))
            .alignment(Alignment::Center);
        let tagline = Paragraph::new(Line::styled(MIRU_TAGLINE, Style::default().fg(Color::DarkGray)))
            .alignment(Alignment::Center);
        
        f.render_widget(logo, logo_area);
        f.render_widget(tagline, tagline_area);
    })?;

    thread::sleep(Duration::from_millis(800));

    // Erase animation - delete letters in reverse
    for i in (0..4).rev() {
        terminal.draw(|f| {
            let area = f.area();
            let frame_text = MIRU_FRAMES[i];
            let lines = frame_text.lines().count() as u16;
            let y_offset = area.height.saturating_sub(lines) / 2;
            
            let centered_area = Rect {
                x: 0,
                y: y_offset,
                width: area.width,
                height: lines + 2,
            };
            
            let para = Paragraph::new(Text::styled(frame_text, Style::default().fg(accent)))
                .alignment(Alignment::Center);
            f.render_widget(para, centered_area);
        })?;
        
        thread::sleep(Duration::from_millis(80));
    }

    // Clear screen
    terminal.draw(|_f| {})?;
    thread::sleep(Duration::from_millis(100));

    Ok(())
}
