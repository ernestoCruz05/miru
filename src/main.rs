mod app;
mod compression;
mod config;
mod error;
mod library;
mod nyaa;
mod player;
mod torrent;
mod ui;
mod rpc;

use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::app::App;
use crate::config::Config;
use crate::error::Result;
use crate::library::Library;

fn setup_logging() -> Result<()> {
    let data_dir = config::data_dir()?;
    std::fs::create_dir_all(&data_dir)?;

    let file_appender = tracing_appender::rolling::daily(&data_dir, "miru.log");

    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env().add_directive("miru=info".parse().unwrap()))
        .with(fmt::layer().with_writer(file_appender).with_ansi(false))
        .init();

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Set up file-based logging (we own the terminal for TUI)
    if let Err(e) = setup_logging() {
        eprintln!("Warning: Could not set up logging: {}", e);
    }

    info!("Starting miru");

    // Load configuration
    let config = Config::load()?;
    info!("Loaded config");

    // Load existing library or create new
    let mut library = Library::load()?;
    info!(shows = library.shows.len(), "Loaded library");

    // Scan for new shows on startup
    let media_dirs = config.expanded_media_dirs();
    library.refresh(&media_dirs)?;
    library.save()?;
    info!(shows = library.shows.len(), "Library refreshed");

    // Initialize terminal
    let mut terminal = app::init_terminal()?;

    // Play splash animation
    let accent = ui::widgets::parse_accent_color(&config.ui.accent_color);
    let _ = app::play_splash(&mut terminal, accent);

    // Run the app (async)
    let mut app = App::new(config, library);
    let result = app.run(&mut terminal).await;

    // Restore terminal on exit
    app::restore_terminal()?;

    // Save library state
    app.library.save()?;

    result
}
