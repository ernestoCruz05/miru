use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use regex::Regex;

use crate::nyaa::{NyaaCategory, NyaaFilter, NyaaResult, NyaaSort};

use super::widgets::titled_block;

fn truncate_title(title: &str, max_width: usize) -> String {
    if title.is_empty() {
        return "Unknown".to_string();
    }

    if max_width <= 3 {
        return "...".to_string();
    }

    if title.len() <= max_width {
        return title.to_string();
    }

    // Episode patterns: - 01, E01, EP01, Episode 5, S01E05
    let episode_patterns = [
        r"- (\d{2,3})",       // " - 01", " - 123"
        r"\bE(\d{2,3})\b",    // "E01", "E123"
        r"\bEP(\d{2,3})\b",   // "EP01", "EP123"
        r"\bEpisode (\d+)\b", // "Episode 5"
        r"\b(S\d+E\d+)\b",    // "S01E05"
    ];

    // Try to find episode info
    for pattern in &episode_patterns {
        if let Ok(re) = Regex::new(pattern) {
            if let Some(cap) = re.find(title) {
                let episode_start = cap.start();
                let episode_end = cap.end();
                let episode_part = &title[episode_start..episode_end];

                // Calculate how much title we can keep before the episode
                // Reserve space for "..." + episode_part
                let reserved = 3 + episode_part.len();
                if reserved >= max_width {
                    // Can't fit everything, just show truncated episode
                    return format!("...{}", episode_part);
                }

                let available_for_title = max_width - reserved;
                let title_prefix = if episode_start > available_for_title {
                    &title[..available_for_title]
                } else {
                    &title[..episode_start]
                };

                return format!("{}...{}", title_prefix.trim_end(), episode_part);
            }
        }
    }

    // No episode pattern found, simple truncation
    format!("{}...", &title[..max_width - 3])
}

pub fn render_search_view(
    frame: &mut Frame,
    area: Rect,
    query: &str,
    results: &[NyaaResult],
    list_state: &mut ListState,
    is_loading: bool,
    category: NyaaCategory,
    filter: NyaaFilter,
    sort: NyaaSort,
    accent: Color,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(3),
        ])
        .split(area);

    render_search_input(frame, chunks[0], query, is_loading, accent);

    render_filter_bar(frame, chunks[1], category, filter, sort);

    render_search_results(frame, chunks[2], results, list_state, accent);
}

fn render_search_input(
    frame: &mut Frame,
    area: Rect,
    query: &str,
    is_loading: bool,
    accent: Color,
) {
    let title = if is_loading {
        " Search nyaa.si (loading...) "
    } else {
        " Search nyaa.si "
    };

    let input = Paragraph::new(query)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(accent))
                .title(title)
                .title_style(Style::default().fg(accent).add_modifier(Modifier::BOLD)),
        )
        .style(Style::default().fg(Color::White));

    frame.render_widget(input, area);

    frame.set_cursor_position((area.x + query.len() as u16 + 1, area.y + 1));
}

fn render_filter_bar(
    frame: &mut Frame,
    area: Rect,
    category: NyaaCategory,
    filter: NyaaFilter,
    sort: NyaaSort,
) {
    let line = Line::from(vec![
        Span::raw(" "),
        Span::styled("c", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(":Category "),
        Span::styled(
            format!("[{}]", category.as_display()),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw("  "),
        Span::styled("f", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(":Filter "),
        Span::styled(
            format!("[{}]", filter.as_display()),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw("  "),
        Span::styled("s", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(":Sort "),
        Span::styled(
            format!("[{}]", sort.as_display()),
            Style::default().fg(Color::LightMagenta),
        ),
    ]);

    let bar = Paragraph::new(line);
    frame.render_widget(bar, area);
}

fn render_search_results(
    frame: &mut Frame,
    area: Rect,
    results: &[NyaaResult],
    list_state: &mut ListState,
    accent: Color,
) {
    if results.is_empty() {
        let empty = Paragraph::new("No results. Type to search, Enter to submit.")
            .block(titled_block("Results", accent))
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(empty, area);
        return;
    }

    let title_width = area.width.saturating_sub(36) as usize;

    let items: Vec<ListItem> = results
        .iter()
        .map(|r| {
            let seeder_color = if r.seeders >= 50 {
                Color::Green
            } else if r.seeders >= 10 {
                Color::Yellow
            } else if r.seeders > 0 {
                Color::Red
            } else {
                Color::DarkGray
            };

            let seeder_style = if r.is_trusted {
                Style::default()
                    .fg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(seeder_color)
                    .add_modifier(Modifier::BOLD)
            };

            let trust_indicator = if r.is_trusted {
                Span::styled("★ ", Style::default().fg(Color::LightGreen))
            } else {
                Span::raw("  ")
            };

            let batch_indicator = if r.is_batch {
                Span::styled("[BATCH] ", Style::default().fg(Color::LightYellow))
            } else {
                Span::raw("        ")
            };

            let title_style = if r.is_trusted || r.is_batch {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            };

            let line = Line::from(vec![
                trust_indicator,
                Span::styled(format!("{:>4}", r.seeders), seeder_style),
                Span::raw(" │ "),
                Span::styled(format!("{:>9}", r.size), Style::default().fg(Color::Cyan)),
                Span::raw(" │ "),
                batch_indicator,
                Span::styled(truncate_title(&r.title, title_width), title_style),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(titled_block("Results", accent))
        .highlight_style(
            Style::default()
                .bg(accent)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, area, list_state);
}
