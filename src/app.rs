use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout},
    style::Color,
    widgets::ListState,
};
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::compression;
use crate::config::Config;
use crate::error::Result;
use crate::library::models::TrackedSeries;
use crate::library::{
    Library,
    tracking::{self, UpdateResult},
};
use crate::nyaa::{NyaaCategory, NyaaClient, NyaaFilter, NyaaResult, NyaaSort};
use crate::player::ExternalPlayer;
use crate::rpc::DiscordRpc;
use crate::torrent::{AnyTorrentClient, QBittorrentClient, TorrentStatus, TransmissionClient};
use crate::ui::{
    render_downloads_view, render_episodes_view, render_library_view, render_search_view, widgets,
};

const VIDEO_EXTENSIONS: &[&str] = &["mkv", "mp4", "avi", "webm", "m4v", "mov", "wmv"];

/// Clean up a torrent filename to a more readable format
/// e.g., "[SubGroup] Show Name - 01 (1080p) [HASH].mkv" -> "Show Name - S01E01.mkv"
fn clean_filename(name: &str) -> String {
    let mut clean = name.to_string();

    // Remove [...] bracketed content (subgroup, hash, quality info)
    while let (Some(start), Some(end)) = (clean.find('['), clean.find(']')) {
        if start < end {
            clean = format!("{}{}", &clean[..start], &clean[end + 1..]);
        } else {
            break;
        }
    }

    // Remove (...) parenthetical content (resolution, codec info)
    while let (Some(start), Some(end)) = (clean.find('('), clean.find(')')) {
        if start < end {
            clean = format!("{}{}", &clean[..start], &clean[end + 1..]);
        } else {
            break;
        }
    }

    clean = clean
        .replace("  ", " ")
        .replace("..", ".")
        .trim()
        .to_string();

    // Try to extract episode number from common patterns
    let episode_patterns = [
        (
            regex::Regex::new(r"[Ss](\d{1,2})[Ee](\d{1,3})").unwrap(),
            true,
        ), // S01E01
        (
            regex::Regex::new(r"[Ee][Pp]?\.?\s*(\d{1,3})").unwrap(),
            false,
        ), // E01, EP01, Ep 01
        (regex::Regex::new(r"\s-\s*(\d{1,3})\b").unwrap(), false), // - 01
        (regex::Regex::new(r"#(\d{1,3})").unwrap(), false),        // #01
    ];

    for (re, has_season) in &episode_patterns {
        if let Some(caps) = re.captures(&clean) {
            if *has_season {
                let season: u32 = caps.get(1).unwrap().as_str().parse().unwrap_or(1);
                let episode: u32 = caps.get(2).unwrap().as_str().parse().unwrap_or(1);
                let show_name = clean[..caps.get(0).unwrap().start()].trim();
                let show_name = show_name.trim_end_matches(&['-', '.', ' '][..]);
                let show_name = show_name.replace('/', "-").replace('\\', "-");
                let ext = Path::new(name)
                    .extension()
                    .and_then(|e| e.to_str())
                    .filter(|e| VIDEO_EXTENSIONS.contains(&e.to_lowercase().as_str()))
                    .unwrap_or("mkv");
                return format!("{} - S{:02}E{:02}.{}", show_name, season, episode, ext);
            } else {
                let episode: u32 = caps.get(1).unwrap().as_str().parse().unwrap_or(1);
                let show_name = clean[..caps.get(0).unwrap().start()].trim();
                let show_name = show_name.trim_end_matches(&['-', '.', ' '][..]);
                let show_name = show_name.replace('/', "-").replace('\\', "-");
                let ext = Path::new(name)
                    .extension()
                    .and_then(|e| e.to_str())
                    .filter(|e| VIDEO_EXTENSIONS.contains(&e.to_lowercase().as_str()))
                    .unwrap_or("mkv");
                return format!("{} - E{:02}.{}", show_name, episode, ext);
            }
        }
    }
    let clean_name = clean
        .replace('/', "-")
        .replace('\\', "-")
        .trim()
        .to_string();
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| VIDEO_EXTENSIONS.contains(&e.to_lowercase().as_str()));

    match ext {
        Some(e) => {
            if clean_name
                .to_lowercase()
                .ends_with(&format!(".{}", e.to_lowercase()))
            {
                clean_name
            } else {
                format!("{}.{}", clean_name, e)
            }
        }
        None => clean_name,
    }
}

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

    Err(crate::error::Error::Io(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("No video file found in {:?}", dir),
    )))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrackingDialogStep {
    Query,
    Group,
    Quality,
    Confirm,
}

pub struct TrackingDialogState {
    pub step: TrackingDialogStep,
    pub input_query: String,
    pub input_group: String,
    pub input_quality: String,
}

impl Default for TrackingDialogState {
    fn default() -> Self {
        Self {
            step: TrackingDialogStep::Query,
            input_query: String::new(),
            input_group: String::new(),
            input_quality: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum View {
    Library,
    Episodes,
    Search,
    Downloads,
    MoveDialog,
    TrackingDialog,
    DeleteDialog,
    Help,
    TrackingList,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeleteTarget {
    Show(usize),
    Episode(usize, usize),
}

pub struct DeleteDialogState {
    pub target: DeleteTarget,
    pub name: String,
}

impl Default for DeleteDialogState {
    fn default() -> Self {
        Self {
            target: DeleteTarget::Show(0),
            name: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveDialogStep {
    SelectMediaDir,
    SelectShow,
    BatchPreview,
    EditFilename,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BatchMoveStrategy {
    #[default]
    PreserveStructure,
    Flatten,
}

impl BatchMoveStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            BatchMoveStrategy::PreserveStructure => "Preserve Structure",
            BatchMoveStrategy::Flatten => "Flatten All",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            BatchMoveStrategy::PreserveStructure => BatchMoveStrategy::Flatten,
            BatchMoveStrategy::Flatten => BatchMoveStrategy::PreserveStructure,
        }
    }
}

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
    pub batch_analysis: Option<crate::library::batch::BatchAnalysis>,
    pub batch_strategy: BatchMoveStrategy,
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
            batch_analysis: None,
            batch_strategy: BatchMoveStrategy::default(),
        }
    }
}

pub enum AppMessage {
    SearchResults(Vec<NyaaResult>),
    SearchError(String),
    TorrentAdded(String),
    TorrentError(String),
    MetadataFound(String, crate::metadata::AnimeMetadata),
    CoverUpdated(String),
    MetadataError(String),
    TorrentList(Vec<TorrentStatus>),
    UpdatesFound(Vec<UpdateResult>),
    AutoSave,
}

pub struct App {
    pub config: Config,
    pub library: Library,
    pub running: bool,
    pub view: View,
    pub previous_view: View,
    pub accent: Color,

    pub library_state: ListState,
    pub tracking_list_state: ListState,
    pub episodes_state: ListState,
    pub selected_show_idx: Option<usize>,

    pub search_query: String,
    pub search_results: Vec<NyaaResult>,
    pub filtered_search_results: Vec<usize>,
    pub search_filter_input: String,
    pub is_filtering: bool,
    pub search_state: ListState,
    pub search_loading: bool,
    pub search_category: NyaaCategory,
    pub search_filter: NyaaFilter,
    pub search_sort: NyaaSort,

    pub torrents: Vec<TorrentStatus>,
    pub downloads_state: ListState,

    pub move_dialog: MoveDialogState,
    pub tracking_state: TrackingDialogState,
    pub delete_dialog_state: DeleteDialogState,

    pub msg_tx: mpsc::UnboundedSender<AppMessage>,
    pub msg_rx: mpsc::UnboundedReceiver<AppMessage>,

    pub nyaa_client: Arc<NyaaClient>,
    pub torrent_client: Option<Arc<AnyTorrentClient>>,
    pub metadata_provider: Option<Arc<dyn crate::metadata::MetadataProvider + Send + Sync>>,
    pub image_cache: Arc<crate::image_cache::ImageCache>,
    pub picker: ratatui_image::picker::Picker,
    pub rpc: Option<DiscordRpc>,
    pub managed_daemon_handle: Option<std::process::Child>,
    pub startup_scan_completed: bool,
    pub dirty: bool,
}

impl App {
    pub fn new(config: Config, library: Library, picker: ratatui_image::picker::Picker) -> Self {
        let accent = widgets::parse_accent_color(&config.ui.accent_color);

        let mut library_state = ListState::default();
        if !library.shows.is_empty() {
            library_state.select(Some(0));
        }

        let (msg_tx, msg_rx) = mpsc::unbounded_channel();

        let torrent_client = create_torrent_client(&config);

        let metadata_provider: Option<Arc<dyn crate::metadata::MetadataProvider + Send + Sync>> =
            if !config.metadata.mal_client_id.is_empty() {
                Some(Arc::new(crate::metadata::mal::MalClient::new(
                    config.metadata.mal_client_id.clone(),
                )))
            } else {
                None
            };

        let image_cache = Arc::new(crate::image_cache::ImageCache::new().unwrap_or_else(|e| {
            tracing::error!("Failed to initialize image cache: {}", e);
            panic!("Failed to initialize image cache: {}", e);
        }));

        Self {
            config,
            library,
            running: true,
            view: View::Library,
            previous_view: View::Library,
            accent,

            library_state,
            tracking_list_state: ListState::default(),
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
            tracking_state: TrackingDialogState::default(),
            delete_dialog_state: DeleteDialogState::default(),

            msg_tx,
            msg_rx,

            nyaa_client: Arc::new(NyaaClient::new()),
            torrent_client: torrent_client.map(Arc::new),
            metadata_provider,
            image_cache,
            picker,
            rpc: Some(DiscordRpc::new("1465518237599928381")),
            managed_daemon_handle: None,
            startup_scan_completed: false,
            dirty: false,
        }
    }

    pub async fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        self.refresh_torrent_list();

        self.spawn_managed_daemon();

        let auto_save_tx = self.msg_tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            interval.tick().await;
            loop {
                interval.tick().await;
                if auto_save_tx.send(AppMessage::AutoSave).is_err() {
                    break;
                }
            }
        });

        while self.running {
            terminal.draw(|frame| self.render(frame))?;
            self.handle_events().await?;
            self.process_messages();
        }

        self.cleanup();
        Ok(())
    }

    fn process_messages(&mut self) {
        while let Ok(msg) = self.msg_rx.try_recv() {
            match msg {
                AppMessage::SearchResults(results) => {
                    self.search_loading = false;
                    self.search_results = results;
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
                AppMessage::TorrentError(e) => {
                    error!("Torrent client error: {}", e);
                }
                AppMessage::MetadataFound(show_id, metadata) => {
                    if let Some(show) = self.library.shows.iter_mut().find(|s| s.id == show_id) {
                        info!("Updated metadata for: {}", show.title);

                        if let Some(url) = metadata.cover_url.clone() {
                            let cache = self.image_cache.clone();
                            let tx = self.msg_tx.clone();
                            let s_id = show_id.clone();
                            tokio::spawn(async move {
                                if let Err(e) = cache.download(&url).await {
                                    tracing::error!("Failed to download cover for {}: {}", s_id, e);
                                } else {
                                    let _ = tx.send(AppMessage::CoverUpdated(s_id));
                                }
                            });
                        }

                        show.metadata = Some(metadata);
                        self.dirty = true;
                        let _ = self.library.save();
                    }
                }
                AppMessage::CoverUpdated(show_id) => {
                    info!("Cover image updated for show: {}", show_id);
                }
                AppMessage::MetadataError(e) => {
                    error!("Metadata fetch failed: {}", e);
                }
                AppMessage::TorrentList(torrents) => {
                    self.torrents = torrents;
                    if !self.torrents.is_empty() && self.downloads_state.selected().is_none() {
                        self.downloads_state.select(Some(0));
                    }

                    if !self.startup_scan_completed {
                        self.startup_scan_completed = true;
                        self.check_for_updates();
                    }
                }
                AppMessage::UpdatesFound(updates) => {
                    for update in updates {
                        let already_active = self
                            .torrents
                            .iter()
                            .any(|t| t.name.to_lowercase() == update.title.to_lowercase());
                        if already_active {
                            debug!("Skipping auto-download (already active): {}", update.title);
                            continue;
                        }

                        if let Some(client) = &self.torrent_client {
                            info!(
                                "Auto-downloading: {} - {}",
                                update.series_title, update.title
                            );
                            let client = client.clone();
                            let magnet = update.magnet.clone();
                            let tx = self.msg_tx.clone();
                            tokio::spawn(async move {
                                match client.add_magnet(&magnet).await {
                                    Ok(_) => {
                                        let _ = tx.send(AppMessage::TorrentAdded(magnet));
                                    }
                                    Err(e) => {
                                        let _ = tx.send(AppMessage::TorrentError(e.to_string()));
                                    }
                                }
                            });
                        }
                    }
                }
                AppMessage::AutoSave => {
                    if self.dirty {
                        if let Err(e) = self.library.save() {
                            error!("Auto-save failed: {}", e);
                        } else {
                            debug!("Auto-save completed");
                            self.dirty = false;
                        }
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
                    &self.image_cache,
                    &mut self.picker,
                );

                let help = widgets::help_bar(&[("?", "help"), ("q", "quit")]);
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

                let help = widgets::help_bar(&[("?", "help"), ("Esc", "back")]);
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

                let help = widgets::help_bar(&[("?", "help"), ("Esc", "back")]);
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

                let help = widgets::help_bar(&[("?", "help"), ("Esc", "back")]);
                frame.render_widget(help, help_area);
            }
            View::MoveDialog => {
                render_downloads_view(
                    frame,
                    main_area,
                    &self.torrents,
                    &mut self.downloads_state,
                    self.accent,
                );

                self.render_move_dialog(frame);

                let help_text = match self.move_dialog.step {
                    MoveDialogStep::SelectMediaDir => {
                        &[("j/k", "navigate"), ("Enter", "select"), ("Esc", "cancel")][..]
                    }
                    MoveDialogStep::SelectShow => &[
                        ("j/k", "navigate"),
                        ("Enter", "select"),
                        ("n", "new folder"),
                        ("Esc", "back"),
                    ][..],
                    MoveDialogStep::BatchPreview => &[
                        ("Tab/s", "change strategy"),
                        ("Enter", "move"),
                        ("Esc", "back"),
                    ][..],
                    MoveDialogStep::EditFilename => &[("Enter", "confirm"), ("Esc", "back")][..],
                };
                let help = widgets::help_bar(help_text);
                frame.render_widget(help, help_area);
            }
            View::TrackingDialog => {
                render_library_view(
                    frame,
                    main_area,
                    &self.library.shows,
                    &mut self.library_state,
                    self.accent,
                    &self.image_cache,
                    &mut self.picker,
                );
                self.render_tracking_dialog(frame);

                let help = widgets::help_bar(&[("Enter", "next/confirm"), ("Esc", "cancel")]);
                frame.render_widget(help, help_area);
            }
            View::DeleteDialog => {
                match self.delete_dialog_state.target {
                    DeleteTarget::Show(_) => {
                        render_library_view(
                            frame,
                            main_area,
                            &self.library.shows,
                            &mut self.library_state,
                            self.accent,
                            &self.image_cache,
                            &mut self.picker,
                        );
                    }
                    DeleteTarget::Episode(idx, _) => {
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
                }
                self.render_delete_dialog(frame);
                let help = widgets::help_bar(&[("Enter", "confirm delete"), ("Esc", "cancel")]);
                frame.render_widget(help, help_area);
            }
            View::TrackingList => {
                self.render_tracking_list(frame, main_area);
                let help = widgets::help_bar(&[("?", "help"), ("x", "untrack"), ("Esc", "back")]);
                frame.render_widget(help, help_area);
            }
            View::Help => {
                match self.previous_view {
                    View::Library => render_library_view(
                        frame,
                        main_area,
                        &self.library.shows,
                        &mut self.library_state,
                        self.accent,
                        &self.image_cache,
                        &mut self.picker,
                    ),
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
                    }
                    View::Search => render_search_view(
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
                    ),
                    View::Downloads => render_downloads_view(
                        frame,
                        main_area,
                        &self.torrents,
                        &mut self.downloads_state,
                        self.accent,
                    ),
                    View::TrackingList => self.render_tracking_list(frame, main_area),
                    _ => {}
                }
                self.render_help(frame);
            }
        }
    }

    async fn handle_events(&mut self) -> Result<()> {
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    return Ok(());
                }
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    self.running = false;
                    return Ok(());
                }

                match self.view {
                    View::Library => self.handle_library_input(key.code)?,
                    View::Episodes => self.handle_episodes_input(key.code)?,
                    View::Search => self.handle_search_input(key)?,
                    View::Downloads => self.handle_downloads_input(key.code).await?,
                    View::MoveDialog => self.handle_move_dialog_input(key.code)?,
                    View::TrackingDialog => self.handle_tracking_input(key.code).await?,
                    View::DeleteDialog => self.handle_delete_dialog_input(key.code)?,
                    View::Help => self.handle_help_input(key.code)?,
                    View::TrackingList => self.handle_tracking_list_input(key.code)?,
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
            KeyCode::Char('m') => {
                if let Some(idx) = self.library_state.selected() {
                    if let Some(show) = self.library.shows.get(idx) {
                        if let Some(provider) = self.metadata_provider.clone() {
                            let show_id = show.id.clone();
                            let query = show.title.clone();
                            let tx = self.msg_tx.clone();

                            info!("Fetching metadata for: {}", query);

                            tokio::spawn(async move {
                                match provider.search(&query).await {
                                    Ok(results) => {
                                        if let Some(first) = results.into_iter().next() {
                                            let _ =
                                                tx.send(AppMessage::MetadataFound(show_id, first));
                                        } else {
                                            let _ = tx.send(AppMessage::MetadataError(format!(
                                                "No results for: {}",
                                                query
                                            )));
                                        }
                                    }
                                    Err(e) => {
                                        let _ = tx.send(AppMessage::MetadataError(e.to_string()));
                                    }
                                }
                            });
                        } else {
                            error!("No metadata provider configured (check mal_client_id)");
                        }
                    }
                }
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
            KeyCode::Char('t') => {
                self.open_tracking_dialog();
            }
            KeyCode::Char('x') => {
                self.open_delete_show_dialog();
            }
            KeyCode::Char('T') => {
                self.view = View::TrackingList;
                if !self.library.tracked_shows.is_empty() {
                    self.tracking_list_state.select(Some(0));
                }
            }
            KeyCode::Char('?') => {
                self.toggle_help();
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
            KeyCode::Char('x') => {
                self.open_delete_episode_dialog();
            }
            KeyCode::Char('?') => {
                self.toggle_help();
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
                    self.search_category = self.search_category.next();
                }
                KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.search_filter = self.search_filter.next();
                }
                KeyCode::Tab | KeyCode::Down => {
                    if !self.search_results.is_empty() {
                        self.move_selection_down(&View::Search);
                    }
                }
                KeyCode::Up => {
                    self.move_selection_up(&View::Search);
                }
                KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.search_sort = self.search_sort.next();
                    if !self.search_results.is_empty() {
                        self.perform_search();
                    }
                }
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
                        self.perform_search();
                    } else {
                        self.download_selected_torrent();
                    }
                }
                KeyCode::Char('?') => {
                    self.toggle_help();
                }
                KeyCode::Char(c) => {
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    {
                        self.search_query.push(c);
                    }
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
            self.filtered_search_results = self
                .search_results
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
            KeyCode::Char('t') => {
                self.open_tracking_dialog();
            }
            KeyCode::Enter => {
                self.play_selected_download()?;
            }
            KeyCode::Char('?') => {
                self.toggle_help();
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
            View::TrackingDialog | View::DeleteDialog | View::Help | View::TrackingList => return,
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
            View::TrackingDialog | View::DeleteDialog | View::Help | View::TrackingList => return,
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
        let show_title = show.title.clone();
        let episode_number = episode.number;

        let (play_path, temp_path) = if compression::is_compressed(&path) {
            info!(path = %path.display(), "Decompressing episode for playback");
            let temp = compression::decompress_to_temp(&path)?;
            (temp.clone(), Some(temp))
        } else {
            (path, None)
        };

        let player_cmd = self.config.general.player.clone();

        let args = if player_cmd == "vlc" {
            self.config
                .player
                .vlc
                .as_ref()
                .map(|p| p.args.clone())
                .unwrap_or_else(|| vec!["--fullscreen".to_string()])
        } else {
            self.config.player.mpv.args.clone()
        };

        let mut player = ExternalPlayer::new(player_cmd, args);

        if let Some(rpc) = &mut self.rpc {
            let details = format!("Watching {} on miru", show_title);
            let state = format!("Episode {}", episode_number);
            rpc.set_activity(&state, &details);
        }

        player.play(&play_path, start_pos)?;

        let mut last_position: Option<u64> = None;
        let mut last_duration: u64 = 0;
        while player.is_running() {
            if let Some(pos) = player.get_position() {
                last_position = Some(pos);
            }
            if let Some(dur) = player.get_duration() {
                last_duration = dur;
            }
            std::thread::sleep(std::time::Duration::from_millis(1000));
        }
        player.wait()?;

        if let Some(rpc) = &mut self.rpc {
            rpc.clear();
        }
        if let Some(temp) = temp_path {
            if let Some(parent) = temp.parent() {
                let _ = std::fs::remove_dir_all(parent);
            }
        }

        // Save position or mark watched based on how far they got
        if let Some(pos) = last_position {
            if last_duration > 0 && pos > last_duration.saturating_sub(120) {
                // Within 2 minutes of end - mark as watched
                self.library.mark_watched(&show_id, episode_number);
            } else if pos > 10 {
                // Only save if they watched more than 10 seconds
                self.library.update_position(&show_id, episode_number, pos);
            }
        } else {
            // No IPC (not mpv or socket failed) - mark as watched
            self.library.mark_watched(&show_id, episode_number);
        }
        self.dirty = true;
        self.library.save()?;
        self.dirty = false;

        Ok(())
    }

    fn play_next_unwatched(&mut self) -> Result<()> {
        let Some(show_idx) = self.library_state.selected() else {
            return Ok(());
        };

        let (show_id, show_title, episode_number, path, start_pos) = {
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
            (
                show.id.clone(),
                show.title.clone(),
                episode.number,
                path,
                start_pos,
            )
        };

        let (play_path, temp_path) = if compression::is_compressed(&path) {
            info!(path = %path.display(), "Decompressing episode for playback");
            let temp = compression::decompress_to_temp(&path)?;
            (temp.clone(), Some(temp))
        } else {
            (path, None)
        };

        let player_cmd = self.config.general.player.clone();

        let args = if player_cmd == "vlc" {
            self.config
                .player
                .vlc
                .as_ref()
                .map(|p| p.args.clone())
                .unwrap_or_else(|| vec!["--fullscreen".to_string()])
        } else {
            self.config.player.mpv.args.clone()
        };

        let mut player = ExternalPlayer::new(player_cmd, args);

        if let Some(rpc) = &mut self.rpc {
            let details = format!("Watching {} on Miru", show_title);
            let state = format!("Episode {}", episode_number);
            rpc.set_activity(&state, &details);
        }

        player.play(&play_path, start_pos)?;

        let mut last_position: Option<u64> = None;
        let mut last_duration: u64 = 0;
        while player.is_running() {
            if let Some(pos) = player.get_position() {
                last_position = Some(pos);
            }
            if let Some(dur) = player.get_duration() {
                last_duration = dur;
            }
            std::thread::sleep(std::time::Duration::from_millis(1000));
        }
        player.wait()?;

        if let Some(rpc) = &mut self.rpc {
            rpc.clear();
        }
        if let Some(temp) = temp_path {
            if let Some(parent) = temp.parent() {
                let _ = std::fs::remove_dir_all(parent);
            }
        }

        if let Some(pos) = last_position {
            if last_duration > 0 && pos > last_duration.saturating_sub(120) {
                self.library.mark_watched(&show_id, episode_number);
            } else if pos > 10 {
                self.library.update_position(&show_id, episode_number, pos);
            }
        } else {
            self.library.mark_watched(&show_id, episode_number);
        }
        self.dirty = true;
        self.library.save()?;
        self.dirty = false;

        Ok(())
    }

    fn download_selected_torrent(&mut self) {
        let Some(idx) = self.search_state.selected() else {
            return;
        };

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

        if torrent.progress < 1.0 {
            debug!("Torrent not complete, cannot play");
            return Ok(());
        }

        let content_path = std::path::Path::new(&torrent.content_path);

        let video_path = if content_path.is_file() {
            content_path.to_path_buf()
        } else if content_path.is_dir() {
            find_video_in_dir(content_path)?
        } else {
            debug!("Content path doesn't exist: {:?}", content_path);
            return Ok(());
        };

        let player_cmd = self.config.general.player.clone();
        let args = if player_cmd == "vlc" {
            self.config
                .player
                .vlc
                .as_ref()
                .map(|p| p.args.clone())
                .unwrap_or_else(|| vec!["--fullscreen".to_string()])
        } else {
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
                    self.dirty = true;
                    let _ = self.library.save();
                }
            }
        }
    }

    fn refresh_library(&mut self) -> Result<()> {
        let media_dirs = self.config.expanded_media_dirs();
        self.library.refresh(&media_dirs)?;
        self.dirty = true;
        self.library.save()?;
        self.dirty = false;

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
            if let Err(e) = client.remove(&hash, true).await {
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

        if idx >= self.torrents.len() {
            return;
        }

        let original_filename = &self.torrents[idx].name;

        // Clean up filename for suggest new show name
        let clean_name = clean_filename(original_filename);
        let media_dirs: Vec<PathBuf> = self.config.expanded_media_dirs();

        let original_path = PathBuf::from(&self.torrents[idx].content_path);

        info!(
            "Opening move dialog. Torrent: '{}', Content Path: '{}', Exists: {}",
            original_filename,
            original_path.display(),
            original_path.exists()
        );

        if !original_path.exists() {
            error!(
                "Content path reported by torrent client does NOT exist: {}",
                original_path.display()
            );
        }

        let batch_analysis = if original_path.is_dir() {
            let analysis = crate::library::batch::analyze_batch(&original_path);
            if analysis.is_batch {
                info!(
                    "Detected batch download: {} videos, {} seasons, specials: {}",
                    analysis.total_videos,
                    analysis.seasons.len(),
                    analysis.specials.total_count()
                );
                Some(analysis)
            } else {
                None
            }
        } else {
            None
        };

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
            new_show_name: clean_name.clone(),
            creating_new: false,
            filename: clean_name.clone(),
            original_path,
            batch_analysis,
            batch_strategy: BatchMoveStrategy::default(),
        };

        self.view = View::MoveDialog;
    }

    fn open_tracking_dialog(&mut self) {
        self.tracking_state = TrackingDialogState::default();
        self.view = View::TrackingDialog;
    }

    async fn handle_tracking_input(&mut self, key: KeyCode) -> Result<()> {
        match key {
            KeyCode::Esc => {
                self.view = View::Library;
            }
            KeyCode::Enter => {
                match self.tracking_state.step {
                    TrackingDialogStep::Query => {
                        if !self.tracking_state.input_query.is_empty() {
                            self.tracking_state.step = TrackingDialogStep::Group;
                        }
                    }
                    TrackingDialogStep::Group => {
                        self.tracking_state.step = TrackingDialogStep::Quality;
                    }
                    TrackingDialogStep::Quality => {
                        self.tracking_state.step = TrackingDialogStep::Confirm;
                    }
                    TrackingDialogStep::Confirm => {
                        // Create and add the tracked series
                        let query = self.tracking_state.input_query.trim().to_string();
                        // Generate ID from query if simple, or just use query as title base
                        let id = crate::library::parser::make_show_id(&query);

                        let season = crate::library::parser::parse_season_number(&query).unwrap_or(1);

                        let series = TrackedSeries {
                            id: id.clone(),
                            title: query.clone(),
                            query: query,
                            filter_group: if self.tracking_state.input_group.trim().is_empty() {
                                None
                            } else {
                                Some(self.tracking_state.input_group.trim().to_string())
                            },
                            filter_quality: if self.tracking_state.input_quality.trim().is_empty() {
                                None
                            } else {
                                Some(self.tracking_state.input_quality.trim().to_string())
                            },
                            min_episode: 0,
                            season,
                            metadata_id: None,
                            cached_metadata: None,
                        };

                        self.library.tracked_shows.push(series);
                        self.dirty = true;
                        self.library.save()?;
                        self.dirty = false;
                        self.view = View::Library;

                        // Trigger check immediately?
                        self.check_for_updates();
                    }
                }
            }
            KeyCode::Backspace => {
                let input = match self.tracking_state.step {
                    TrackingDialogStep::Query => &mut self.tracking_state.input_query,
                    TrackingDialogStep::Group => &mut self.tracking_state.input_group,
                    TrackingDialogStep::Quality => &mut self.tracking_state.input_quality,
                    TrackingDialogStep::Confirm => return Ok(()),
                };
                input.pop();
            }
            KeyCode::Char(c) => {
                let input = match self.tracking_state.step {
                    TrackingDialogStep::Query => &mut self.tracking_state.input_query,
                    TrackingDialogStep::Group => &mut self.tracking_state.input_group,
                    TrackingDialogStep::Quality => &mut self.tracking_state.input_quality,
                    TrackingDialogStep::Confirm => return Ok(()),
                };
                input.push(c);
            }
            _ => {}
        }
        Ok(())
    }

    fn check_for_updates(&self) {
        let library = self.library.clone();
        let client = self.nyaa_client.clone();
        let tx = self.msg_tx.clone();

        let existing_torrents: Vec<tracking::ExistingTorrent> = self
            .torrents
            .iter()
            .map(|t| tracking::ExistingTorrent {
                hash: t.hash.clone(),
                name: t.name.clone(),
            })
            .collect();

        tokio::spawn(async move {
            let updates = tracking::check_for_updates(&library, &client, &existing_torrents).await;
            if !updates.is_empty() {
                let _ = tx.send(AppMessage::UpdatesFound(updates));
            }
        });
    }

    fn handle_move_dialog_input(&mut self, key: KeyCode) -> Result<()> {
        match self.move_dialog.step {
            MoveDialogStep::SelectMediaDir => match key {
                KeyCode::Esc => {
                    self.view = View::Downloads;
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    let len = self.move_dialog.media_dirs.len();
                    if len > 0 {
                        let next = self
                            .move_dialog
                            .media_dir_state
                            .selected()
                            .map(|i| (i + 1).min(len - 1))
                            .unwrap_or(0);
                        self.move_dialog.media_dir_state.select(Some(next));
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    let next = self
                        .move_dialog
                        .media_dir_state
                        .selected()
                        .map(|i| i.saturating_sub(1))
                        .unwrap_or(0);
                    self.move_dialog.media_dir_state.select(Some(next));
                }
                KeyCode::Enter => {
                    if let Some(idx) = self.move_dialog.media_dir_state.selected() {
                        if let Some(dir) = self.move_dialog.media_dirs.get(idx).cloned() {
                            self.move_dialog.selected_media_dir = Some(dir.clone());

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
            },
            MoveDialogStep::SelectShow => {
                if self.move_dialog.creating_new {
                    match key {
                        KeyCode::Esc => {
                            self.move_dialog.creating_new = false;
                            self.move_dialog.new_show_name.clear();
                        }
                        KeyCode::Enter => {
                            if !self.move_dialog.new_show_name.is_empty() {
                                self.move_dialog.selected_show =
                                    Some(self.move_dialog.new_show_name.clone());
                                self.move_dialog.creating_new = false;
                                if self.move_dialog.batch_analysis.is_some() {
                                    self.move_dialog.step = MoveDialogStep::BatchPreview;
                                } else {
                                    self.move_dialog.step = MoveDialogStep::EditFilename;
                                }
                            }
                        }
                        KeyCode::Backspace => {
                            self.move_dialog.new_show_name.pop();
                        }
                        KeyCode::Char(c) => {
                            if c != '/' && c != '\\' {
                                self.move_dialog.new_show_name.push(c);
                            }
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
                                let next = self
                                    .move_dialog
                                    .show_state
                                    .selected()
                                    .map(|i| (i + 1).min(len - 1))
                                    .unwrap_or(0);
                                self.move_dialog.show_state.select(Some(next));
                            }
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            let next = self
                                .move_dialog
                                .show_state
                                .selected()
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
                                if let Some(show) = self.move_dialog.shows_in_dir.get(idx).cloned()
                                {
                                    self.move_dialog.selected_show = Some(show);
                                    if self.move_dialog.batch_analysis.is_some() {
                                        self.move_dialog.step = MoveDialogStep::BatchPreview;
                                    } else {
                                        self.move_dialog.step = MoveDialogStep::EditFilename;
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            MoveDialogStep::BatchPreview => match key {
                KeyCode::Esc => {
                    self.move_dialog.step = MoveDialogStep::SelectShow;
                }
                KeyCode::Tab | KeyCode::Char('s') => {
                    self.move_dialog.batch_strategy = self.move_dialog.batch_strategy.next();
                }
                KeyCode::Enter => {
                    if let Err(e) = self.execute_batch_move() {
                        error!(
                            "Failed to move batch: {}. Source may have been deleted or is in use.",
                            e
                        );
                    }
                }
                _ => {}
            },
            MoveDialogStep::EditFilename => match key {
                KeyCode::Esc => {
                    self.move_dialog.step = MoveDialogStep::SelectShow;
                }
                KeyCode::Enter => {
                    if let Err(e) = self.execute_move() {
                        error!(
                            "Failed to move file: {}. Source may have been deleted or is in use.",
                            e
                        );
                    }
                }
                KeyCode::Backspace => {
                    self.move_dialog.filename.pop();
                }
                KeyCode::Char(c) => {
                    self.move_dialog.filename.push(c);
                }
                _ => {}
            },
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

        let is_batch_preview = self.move_dialog.step == MoveDialogStep::BatchPreview;
        let dialog_width = if is_batch_preview { 70 } else { 60 }.min(area.width.saturating_sub(4));
        let dialog_height =
            if is_batch_preview { 20 } else { 15 }.min(area.height.saturating_sub(4));

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
            MoveDialogStep::BatchPreview => "Move to Library - Batch Preview",
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
                let items: Vec<ListItem> = self
                    .move_dialog
                    .media_dirs
                    .iter()
                    .map(|p| ListItem::new(p.display().to_string()))
                    .collect();

                let list = List::new(items)
                    .highlight_style(
                        Style::default()
                            .fg(self.accent)
                            .add_modifier(Modifier::BOLD),
                    )
                    .highlight_symbol("> ");

                frame.render_stateful_widget(
                    list,
                    inner,
                    &mut self.move_dialog.media_dir_state.clone(),
                );
            }
            MoveDialogStep::SelectShow => {
                if self.move_dialog.creating_new {
                    let input_text = format!("> {}_", self.move_dialog.new_show_name);
                    let para = Paragraph::new(input_text).style(Style::default().fg(self.accent));
                    frame.render_widget(para, inner);
                } else {
                    let mut items: Vec<ListItem> = self
                        .move_dialog
                        .shows_in_dir
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
                        .highlight_style(
                            Style::default()
                                .fg(self.accent)
                                .add_modifier(Modifier::BOLD),
                        )
                        .highlight_symbol("> ");

                    frame.render_stateful_widget(
                        list,
                        inner,
                        &mut self.move_dialog.show_state.clone(),
                    );
                }
            }
            MoveDialogStep::BatchPreview => {
                let dest_path = self
                    .move_dialog
                    .selected_media_dir
                    .as_ref()
                    .map(|p| {
                        p.join(
                            self.move_dialog
                                .selected_show
                                .as_ref()
                                .unwrap_or(&String::new()),
                        )
                    })
                    .unwrap_or_default();

                let mut lines = vec![
                    Line::from(vec![
                        Span::styled("Destination: ", Style::default().fg(Color::DarkGray)),
                        Span::raw(dest_path.display().to_string()),
                    ]),
                    Line::from(""),
                    Line::from(vec![Span::styled(
                        "Batch detected! ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )]),
                ];

                if let Some(ref analysis) = self.move_dialog.batch_analysis {
                    lines.push(Line::from(vec![
                        Span::styled("Contents: ", Style::default().fg(Color::DarkGray)),
                        Span::raw(analysis.summary()),
                    ]));
                    lines.push(Line::from(""));

                    if !analysis.seasons.is_empty() {
                        lines.push(Line::from(vec![Span::styled(
                            "Seasons:",
                            Style::default().fg(Color::Cyan),
                        )]));
                        for season in &analysis.seasons {
                            lines.push(Line::from(vec![Span::raw(format!(
                                "  {} - {} episode(s)",
                                season.folder_name,
                                season.episodes.len()
                            ))]));
                        }
                    }

                    if !analysis.specials.is_empty() {
                        lines.push(Line::from(vec![Span::styled(
                            format!("Specials: {} file(s)", analysis.specials.total_count()),
                            Style::default().fg(Color::Magenta),
                        )]));
                    }

                    if !analysis.loose_episodes.is_empty() {
                        lines.push(Line::from(vec![Span::styled(
                            format!("Loose episodes: {}", analysis.loose_episodes.len()),
                            Style::default().fg(Color::DarkGray),
                        )]));
                    }
                }

                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("Strategy: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("[{}]", self.move_dialog.batch_strategy.as_str()),
                        Style::default()
                            .fg(self.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " (Tab or 's' to change)",
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled(
                    "Press Enter to move, Esc to go back",
                    Style::default().fg(Color::DarkGray),
                )]));

                let para = Paragraph::new(lines);
                frame.render_widget(para, inner);
            }
            MoveDialogStep::EditFilename => {
                let dest_path = self
                    .move_dialog
                    .selected_media_dir
                    .as_ref()
                    .map(|p| {
                        p.join(
                            self.move_dialog
                                .selected_show
                                .as_ref()
                                .unwrap_or(&String::new()),
                        )
                    })
                    .unwrap_or_default();

                let lines = vec![
                    Line::from(vec![
                        Span::styled("Destination: ", Style::default().fg(Color::DarkGray)),
                        Span::raw(dest_path.display().to_string()),
                    ]),
                    Line::from(""),
                    Line::from(vec![Span::styled(
                        "Filename: ",
                        Style::default().fg(Color::DarkGray),
                    )]),
                    Line::from(vec![Span::styled(
                        format!("{}_", self.move_dialog.filename),
                        Style::default().fg(self.accent),
                    )]),
                    Line::from(""),
                    Line::from(vec![Span::styled(
                        "Press Enter to confirm, Esc to go back",
                        Style::default().fg(Color::DarkGray),
                    )]),
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

        if !dest_dir.exists() {
            std::fs::create_dir_all(&dest_dir)?;
        }

        let dest_path = dest_dir.join(&self.move_dialog.filename);
        let source_path = &self.move_dialog.original_path;

        let real_source_path = if source_path.is_dir() {
            info!("Source is directory: {}", source_path.display());
            match find_video_in_dir(source_path) {
                Ok(p) => {
                    info!("Found video in dir: {}", p.display());
                    p
                }
                Err(e) => {
                    error!(
                        "Failed to find video in dir {}: {}",
                        source_path.display(),
                        e
                    );
                    return Err(e);
                }
            }
        } else {
            info!("Source is file: {}", source_path.display());
            source_path.clone()
        };

        if !real_source_path.exists() {
            error!(
                "CRITICAL: Source file DOES NOT EXIST at moment of move: {}",
                real_source_path.display()
            );
            if let Some(parent) = real_source_path.parent() {
                if let Ok(entries) = std::fs::read_dir(parent) {
                    let file_list: Vec<String> = entries
                        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().to_string()))
                        .collect();
                    error!(
                        "Parent dir content for {}: {:?}",
                        parent.display(),
                        file_list
                    );
                } else {
                    error!("Could not read parent dir: {}", parent.display());
                }
            }
        } else {
            info!(
                "Source file CONFIRMED exists: {}",
                real_source_path.display()
            );
            if let Ok(meta) = std::fs::metadata(&real_source_path) {
                info!(
                    "Source metadata: is_file={}, len={}, permissions={:?}",
                    meta.is_file(),
                    meta.len(),
                    meta.permissions()
                );
            }
        }

        debug!(
            source = %real_source_path.display(),
            dest = %dest_path.display(),
            "Moving file"
        );

        if std::fs::rename(&real_source_path, &dest_path).is_err() {
            std::fs::copy(&real_source_path, &dest_path)?;
            std::fs::remove_file(&real_source_path)?;
        }
        if self.config.general.compress_episodes {
            info!(path = %dest_path.display(), "Compressing episode");
            compression::compress_file(&dest_path, self.config.general.compression_level)?;
        }

        if let Some(client) = self.torrent_client.clone() {
            if let Some(torrent) = self.torrents.get(self.move_dialog.torrent_idx) {
                let hash = torrent.hash.clone();
                let name = torrent.name.clone();
                let tx = self.msg_tx.clone();
                tokio::spawn(async move {
                    info!("Removing moved torrent from client: {}", name);
                    if let Err(e) = client.remove(&hash, false).await {
                        let _ = tx.send(AppMessage::TorrentError(e.to_string()));
                    }
                });
            }
        }

        self.refresh_library()?;

        self.view = View::Downloads;

        Ok(())
    }

    fn execute_batch_move(&mut self) -> Result<()> {
        let Some(media_dir) = &self.move_dialog.selected_media_dir else {
            return Ok(());
        };
        let Some(show_name) = &self.move_dialog.selected_show else {
            return Ok(());
        };

        let dest_dir = media_dir.join(show_name);
        let source_path = &self.move_dialog.original_path;

        if !dest_dir.exists() {
            std::fs::create_dir_all(&dest_dir)?;
        }

        info!(
            "Executing batch move: {} -> {} (strategy: {:?})",
            source_path.display(),
            dest_dir.display(),
            self.move_dialog.batch_strategy
        );

        match self.move_dialog.batch_strategy {
            BatchMoveStrategy::PreserveStructure => {
                self.move_directory_contents(source_path, &dest_dir)?;
            }
            BatchMoveStrategy::Flatten => {
                self.move_videos_flattened(source_path, &dest_dir)?;
            }
        }

        if self.config.general.compress_episodes {
            self.compress_directory_videos(&dest_dir)?;
        }
        if let Some(client) = self.torrent_client.clone() {
            if let Some(torrent) = self.torrents.get(self.move_dialog.torrent_idx) {
                let hash = torrent.hash.clone();
                let name = torrent.name.clone();
                let tx = self.msg_tx.clone();
                tokio::spawn(async move {
                    info!("Removing moved batch torrent from client: {}", name);
                    if let Err(e) = client.remove(&hash, false).await {
                        let _ = tx.send(AppMessage::TorrentError(e.to_string()));
                    }
                });
            }
        }

        if source_path.is_dir() {
            let _ = std::fs::remove_dir_all(source_path);
        }
        self.refresh_library()?;
        self.view = View::Downloads;

        Ok(())
    }

    fn move_directory_contents(&self, src: &Path, dest: &Path) -> Result<()> {
        self.walk_and_move_recursive(src, dest, src, true)?;
        Ok(())
    }
    fn move_videos_flattened(&self, src: &Path, dest: &Path) -> Result<()> {
        self.walk_and_move_recursive(src, dest, src, false)?;
        Ok(())
    }
    fn walk_and_move_recursive(
        &self,
        current: &Path,
        dest: &Path,
        root: &Path,
        preserve_structure: bool,
    ) -> Result<()> {
        let entries = std::fs::read_dir(current)?;

        for entry in entries.filter_map(|e| e.ok()) {
            let entry_path = entry.path();

            if entry_path.is_dir() {
                self.walk_and_move_recursive(&entry_path, dest, root, preserve_structure)?;
            } else if entry_path.is_file() {
                let filename = entry_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                if crate::library::parser::is_video_file(&filename) {
                    let dest_path = if preserve_structure {
                        let relative = entry_path.strip_prefix(root).unwrap_or(&entry_path);
                        let full_dest = dest.join(relative);
                        if let Some(parent) = full_dest.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        full_dest
                    } else {
                        let base_path = dest.join(&filename);
                        if base_path.exists() {
                            let stem = Path::new(&filename)
                                .file_stem()
                                .unwrap_or_default()
                                .to_string_lossy();
                            let ext = Path::new(&filename)
                                .extension()
                                .map(|e| e.to_string_lossy().to_string())
                                .unwrap_or_default();
                            let mut counter = 1;
                            loop {
                                let new_name = format!("{}_{}.{}", stem, counter, ext);
                                let new_path = dest.join(&new_name);
                                if !new_path.exists() {
                                    break new_path;
                                }
                                counter += 1;
                            }
                        } else {
                            base_path
                        }
                    };

                    if std::fs::rename(&entry_path, &dest_path).is_err() {
                        std::fs::copy(&entry_path, &dest_path)?;
                        std::fs::remove_file(&entry_path)?;
                    }
                    info!("Moved: {} -> {}", entry_path.display(), dest_path.display());
                }
            }
        }
        Ok(())
    }

    fn compress_directory_videos(&self, dir: &Path) -> Result<()> {
        self.compress_videos_recursive(dir)?;
        Ok(())
    }

    fn compress_videos_recursive(&self, dir: &Path) -> Result<()> {
        let entries = std::fs::read_dir(dir)?;

        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();

            if path.is_dir() {
                self.compress_videos_recursive(&path)?;
            } else if path.is_file() {
                let filename = path.to_string_lossy();
                if crate::library::parser::is_video_file(&filename) && !filename.ends_with(".zst") {
                    info!(path = %path.display(), "Compressing episode");
                    compression::compress_file(&path, self.config.general.compression_level)?;
                }
            }
        }
        Ok(())
    }
    fn render_tracking_dialog(&self, frame: &mut Frame) {
        use ratatui::layout::{Constraint, Layout, Rect};
        use ratatui::style::Style;
        use ratatui::text::{Line, Text};
        use ratatui::widgets::{Block, Borders, Clear, Paragraph};

        let area = frame.area();
        let dialog_area = Rect {
            x: area.width.saturating_sub(60) / 2,
            y: area.height.saturating_sub(10) / 2,
            width: 60,
            height: 10,
        };

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .title(" Track New Series ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.accent));

        let inner_area = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let layout = Layout::default()
            .constraints([
                Constraint::Length(2),
                Constraint::Length(3),
                Constraint::Min(1),
            ])
            .split(inner_area);

        let (step_text, input_text) = match self.tracking_state.step {
            TrackingDialogStep::Query => (
                "Step 1/3: Enter Search Query (e.g. 'Fate Strange Fake')",
                self.tracking_state.input_query.as_str(),
            ),
            TrackingDialogStep::Group => (
                "Step 2/3: Enter Release Group Filter (Optional)",
                self.tracking_state.input_group.as_str(),
            ),
            TrackingDialogStep::Quality => (
                "Step 3/3: Enter Quality Filter (Optional)",
                self.tracking_state.input_quality.as_str(),
            ),
            TrackingDialogStep::Confirm => ("Adding series...", ""),
        };

        frame.render_widget(
            Paragraph::new(step_text).style(Style::default().fg(Color::Cyan)),
            layout[0],
        );

        frame.render_widget(
            Paragraph::new(format!("> {}", input_text))
                .style(Style::default().fg(Color::White))
                .block(Block::default().borders(Borders::BOTTOM)),
            layout[1],
        );

        if !self.tracking_state.input_group.is_empty()
            || !self.tracking_state.input_quality.is_empty()
        {
            let summary = format!(
                "Group: {}, Quality: {}",
                if self.tracking_state.input_group.is_empty() {
                    "Any"
                } else {
                    &self.tracking_state.input_group
                },
                if self.tracking_state.input_quality.is_empty() {
                    "Any"
                } else {
                    &self.tracking_state.input_quality
                }
            );
            frame.render_widget(
                Paragraph::new(summary).style(Style::default().fg(Color::DarkGray)),
                layout[2],
            );
        }
    }

    fn spawn_managed_daemon(&mut self) {
        if let Some(cmd) = &self.config.torrent.managed_daemon_command {
            info!("Launching managed daemon: {}", cmd);
            let mut command = std::process::Command::new(cmd);

            if let Some(args) = &self.config.torrent.managed_daemon_args {
                command.args(args);
            }

            command.stdout(std::process::Stdio::null());
            command.stderr(std::process::Stdio::null());

            match command.spawn() {
                Ok(child) => {
                    info!("Daemon launched successfully (PID: {})", child.id());
                    self.managed_daemon_handle = Some(child);
                }
                Err(e) => error!("Failed to launch daemon: {}", e),
            }
        }
    }

    fn cleanup(&mut self) {
        if let Some(mut child) = self.managed_daemon_handle.take() {
            info!("Stopping managed daemon (PID: {})", child.id());

            let pid = child.id().to_string();
            let _ = std::process::Command::new("kill").arg(&pid).output();
        }
    }

    fn open_delete_show_dialog(&mut self) {
        if let Some(idx) = self.library_state.selected() {
            if let Some(show) = self.library.shows.get(idx) {
                self.delete_dialog_state = DeleteDialogState {
                    target: DeleteTarget::Show(idx),
                    name: show.title.clone(),
                };
                self.view = View::DeleteDialog;
            }
        }
    }

    fn open_delete_episode_dialog(&mut self) {
        if let Some(show_idx) = self.selected_show_idx {
            if let Some(ep_idx) = self.episodes_state.selected() {
                if let Some(show) = self.library.shows.get(show_idx) {
                    if let Some(ep) = show.episodes.get(ep_idx) {
                        self.delete_dialog_state = DeleteDialogState {
                            target: DeleteTarget::Episode(show_idx, ep_idx),
                            name: format!("Episode {}", ep.number),
                        };
                        self.view = View::DeleteDialog;
                    }
                }
            }
        }
    }

    fn handle_delete_dialog_input(&mut self, key: KeyCode) -> Result<()> {
        match key {
            KeyCode::Esc => match self.delete_dialog_state.target {
                DeleteTarget::Show(_) => self.view = View::Library,
                DeleteTarget::Episode(_, _) => self.view = View::Episodes,
            },
            KeyCode::Enter => match self.delete_dialog_state.target {
                DeleteTarget::Show(idx) => {
                    if let Some(show) = self.library.shows.get(idx) {
                        info!("Deleting show: {}", show.title);
                        if show.path.exists() {
                            std::fs::remove_dir_all(&show.path)?;
                        }
                        self.library.shows.remove(idx);
                        self.dirty = true;
                        self.library.save()?;
                        self.dirty = false;
                    }
                    self.view = View::Library;
                    self.library_state.select(None);
                }
                DeleteTarget::Episode(show_idx, ep_idx) => {
                    if let Some(show) = self.library.shows.get_mut(show_idx) {
                        if let Some(ep) = show.episodes.get(ep_idx) {
                            let path = ep.full_path(&show.path);
                            info!("Deleting episode file: {:?}", path);
                            if path.exists() {
                                std::fs::remove_file(path)?;
                            }
                            show.episodes.remove(ep_idx);
                        }
                        self.dirty = true;
                        self.library.save()?;
                        self.dirty = false;
                    }
                    self.view = View::Episodes;
                    self.episodes_state.select(None);
                }
            },
            _ => {}
        }
        Ok(())
    }

    fn render_delete_dialog(&self, frame: &mut Frame) {
        use ratatui::layout::{Alignment, Rect};
        use ratatui::style::{Color, Modifier, Style};
        use ratatui::text::{Line, Text};
        use ratatui::widgets::{Block, Borders, Clear, Paragraph};

        let area = frame.area();
        let dialog_area = Rect {
            x: area.width.saturating_sub(50) / 2,
            y: area.height.saturating_sub(6) / 2,
            width: 50,
            height: 6,
        };

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .title(" Confirm Deletion ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red));

        let inner_area = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let text = Text::from(vec![
            Line::from(vec!["Are you sure you want to delete:".into()]),
            Line::from(vec![ratatui::text::Span::styled(
                &self.delete_dialog_state.name,
                Style::default().add_modifier(Modifier::BOLD).fg(Color::Red),
            )]),
            Line::from(""),
            Line::from("This action cannot be undone."),
        ]);

        let para = Paragraph::new(text).alignment(Alignment::Center);
        frame.render_widget(para, inner_area);
    }
    fn toggle_help(&mut self) {
        if self.view == View::Help {
            self.view = self.previous_view;
        } else {
            self.previous_view = self.view;
            self.view = View::Help;
        }
    }

    fn render_help(&self, frame: &mut Frame) {
        use ratatui::layout::{Constraint, Layout, Rect};
        use ratatui::style::{Color, Modifier, Style};
        use ratatui::text::{Line, Text};
        use ratatui::widgets::{Block, Borders, Clear, Paragraph, Row, Table};

        let area = frame.area();
        let dialog_area = Rect {
            x: area.width.saturating_sub(80) / 2,
            y: area.height.saturating_sub(30) / 2,
            width: 80,
            height: 30,
        };

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .title(" Help ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.accent));

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let rows = vec![
            Row::new(vec!["Global", "?", "Toggle Help"]),
            Row::new(vec!["", "q", "Quit"]),
            Row::new(vec!["Library", "j/k", "Navigate"]),
            Row::new(vec!["", "Enter/l", "View Episodes"]),
            Row::new(vec!["", "/", "Search Nyaa"]),
            Row::new(vec!["", "t", "Track New Series"]),
            Row::new(vec!["", "T", "View Tracked Shows"]),
            Row::new(vec!["", "x", "Delete Show"]),
            Row::new(vec!["", "r", "Refresh"]),
            Row::new(vec!["Episodes", "Enter", "Play"]),
            Row::new(vec!["", "Space", "Toggle Watched"]),
            Row::new(vec!["", "x", "Delete Episode"]),
            Row::new(vec!["Search", "Enter", "Download"]),
            Row::new(vec!["", "Tab", "Navigate Results"]),
            Row::new(vec!["Downloads", "p", "Pause/Resume"]),
            Row::new(vec!["", "x", "Remove"]),
            Row::new(vec!["", "m", "Move to Library"]),
        ];

        let table = Table::new(
            rows,
            &[
                Constraint::Percentage(20),
                Constraint::Percentage(20),
                Constraint::Percentage(60),
            ],
        )
        .header(
            Row::new(vec!["Context", "Key", "Action"]).style(
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .fg(self.accent),
            ),
        )
        .block(Block::default().borders(Borders::NONE));

        frame.render_widget(table, inner);
    }

    fn handle_help_input(&mut self, key: KeyCode) -> Result<()> {
        match key {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                self.toggle_help();
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_tracking_list_input(&mut self, key: KeyCode) -> Result<()> {
        match key {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('T') => {
                self.view = View::Library;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let len = self.library.tracked_shows.len();
                if len > 0 {
                    let next = self
                        .tracking_list_state
                        .selected()
                        .map(|i| (i + 1).min(len - 1))
                        .unwrap_or(0);
                    self.tracking_list_state.select(Some(next));
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let next = self
                    .tracking_list_state
                    .selected()
                    .map(|i| i.saturating_sub(1))
                    .unwrap_or(0);
                self.tracking_list_state.select(Some(next));
            }
            KeyCode::Char('x') | KeyCode::Char('d') => {
                if let Some(idx) = self.tracking_list_state.selected() {
                    if idx < self.library.tracked_shows.len() {
                        self.library.tracked_shows.remove(idx);
                        self.dirty = true;
                        self.library.save()?;
                        self.dirty = false;
                        // Adjust selection
                        let len = self.library.tracked_shows.len();
                        if len == 0 {
                            self.tracking_list_state.select(None);
                        } else if idx >= len {
                            self.tracking_list_state.select(Some(len - 1));
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn render_tracking_list(&mut self, frame: &mut Frame, area: ratatui::layout::Rect) {
        use ratatui::style::{Color, Modifier, Style};
        use ratatui::widgets::{Block, Borders, List, ListItem};

        let items: Vec<ListItem> = self
            .library
            .tracked_shows
            .iter()
            .map(|s| {
                let title = format!("{} (Query: {})", s.title, s.query);
                ListItem::new(title)
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(" Tracked Shows ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(self.accent)),
            )
            .highlight_style(
                Style::default()
                    .fg(self.accent)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, area, &mut self.tracking_list_state);
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

pub fn play_splash(terminal: &mut DefaultTerminal, accent: Color) -> io::Result<()> {
    use ratatui::{
        layout::{Alignment, Rect},
        style::Style,
        text::{Line, Text},
        widgets::Paragraph,
    };

    for frame in &MIRU_FRAMES {
        terminal.draw(|f| {
            let area = f.area();
            let text = Text::styled(*frame, Style::default().fg(accent));

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
        let tagline = Paragraph::new(Line::styled(
            MIRU_TAGLINE,
            Style::default().fg(Color::DarkGray),
        ))
        .alignment(Alignment::Center);

        f.render_widget(logo, logo_area);
        f.render_widget(tagline, tagline_area);
    })?;

    thread::sleep(Duration::from_millis(800));

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

    terminal.draw(|_f| {})?;
    thread::sleep(Duration::from_millis(100));

    Ok(())
}
