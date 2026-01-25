use humansize::{format_size, BINARY};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState},
    Frame,
};

use crate::torrent::{TorrentState, TorrentStatus};

use super::widgets::titled_block;

pub fn render_downloads_view(
    frame: &mut Frame,
    area: Rect,
    torrents: &[TorrentStatus],
    list_state: &mut ListState,
    accent: Color,
) {
    if torrents.is_empty() {
        let empty = ratatui::widgets::Paragraph::new("No active downloads")
            .block(titled_block("Downloads", accent))
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(empty, area);
        return;
    }

    let items: Vec<ListItem> = torrents
        .iter()
        .map(|t| {
            let state_color = match t.state {
                TorrentState::Downloading => Color::Green,
                TorrentState::Seeding => Color::Cyan,
                TorrentState::Paused => Color::Yellow,
                TorrentState::Queued => Color::Blue,
                TorrentState::Checking => Color::Magenta,
                TorrentState::Error => Color::Red,
                TorrentState::Unknown => Color::DarkGray,
            };

            let progress_pct = (t.progress * 100.0) as u8;

            // Format download speed
            let speed = if t.download_rate > 0 {
                format!("{}/s", format_size(t.download_rate, BINARY))
            } else {
                String::new()
            };

            // Progress bar using unicode blocks
            let bar_width = 20;
            let filled = ((t.progress * bar_width as f64) as usize).min(bar_width);
            let empty = bar_width - filled;
            let progress_bar = format!(
                "{}{}",
                "█".repeat(filled),
                "░".repeat(empty)
            );

            let line = Line::from(vec![
                Span::styled(
                    format!("{:>3}%", progress_pct),
                    Style::default().fg(state_color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(progress_bar, Style::default().fg(state_color)),
                Span::raw(" "),
                Span::styled(
                    format!("{:>10}", speed),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw(" │ "),
                // Truncate name if too long
                Span::raw(truncate_name(&t.name, 50)),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(titled_block("Downloads", accent))
        .highlight_style(
            Style::default()
                .bg(accent)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, area, list_state);
}

fn truncate_name(name: &str, max_len: usize) -> String {
    if name.len() <= max_len {
        name.to_string()
    } else {
        format!("{}...", &name[..max_len - 3])
    }
}
