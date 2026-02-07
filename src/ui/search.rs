use humansize::{BINARY, format_size};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Flex, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};
use regex::Regex;

use crate::nyaa::{NyaaCategory, NyaaFilter, NyaaResult, NyaaSort};
use crate::torrent::preview::{FileType, PreviewSection, PreviewState, TorrentFileEntry};

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

pub fn render_preview_popup(frame: &mut Frame, preview: &mut PreviewState, accent: Color) {
    let area = frame.area();

    // Build the flat item list to know total height
    let (items, _section_count) = build_file_list_items(preview, accent);
    let file_lines = items.len().max(1) as u16;

    // Dynamic height: borders(2) + file_lines + mal_footer(2) + summary(1) + hints(1)
    let content_height = file_lines + 4;
    let popup_height = (content_height + 2).min(area.height.saturating_sub(4)); // +2 for border
    let popup_width = 72u16.min(area.width.saturating_sub(6));

    let [popup_area] = Layout::horizontal([Constraint::Length(popup_width)])
        .flex(Flex::Center)
        .areas(area);
    let [popup_area] = Layout::vertical([Constraint::Length(popup_height)])
        .flex(Flex::Center)
        .areas(popup_area);

    frame.render_widget(Clear, popup_area);

    let title = if preview.torrent_title.len() > 50 {
        format!(" Preview: {}... ", &preview.torrent_title[..47])
    } else {
        format!(" Preview: {} ", &preview.torrent_title)
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(accent));

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let chunks = Layout::vertical([
        Constraint::Min(3),    // file list
        Constraint::Length(2), // MAL footer
        Constraint::Length(1), // summary line
        Constraint::Length(1), // action hints
    ])
    .split(inner);

    // -- File list section --
    render_file_list(frame, chunks[0], &mut preview.scroll_state, &items, accent);

    // -- MAL footer section --
    render_mal_footer(frame, chunks[1], preview);

    // -- Summary line --
    render_summary_line(frame, chunks[2], preview);

    // -- Action hints --
    let hints = if preview.is_magnet_only {
        "Enter: Download anyway  |  Esc: Close"
    } else {
        "Enter: Download  |  j/k: Scroll  |  Esc: Close"
    };
    let hints_paragraph = Paragraph::new(hints).style(Style::default().fg(Color::DarkGray));
    frame.render_widget(hints_paragraph, chunks[3]);
}

fn build_file_list_items<'a>(preview: &PreviewState, accent: Color) -> (Vec<ListItem<'a>>, usize) {
    match &preview.torrent_files {
        PreviewSection::Loading => {
            let item = ListItem::new(Line::from(Span::styled(
                "\u{25CB} Loading torrent data...",
                Style::default().fg(Color::DarkGray),
            )));
            (vec![item], 0)
        }
        PreviewSection::Error(msg) => {
            let item = ListItem::new(Line::from(Span::styled(
                msg.clone(),
                Style::default().fg(Color::Yellow),
            )));
            (vec![item], 0)
        }
        PreviewSection::Loaded(files) => {
            let mut items = Vec::new();
            let mut sections = 0;

            // Group by type: Video first, then Subtitles, then Other
            let videos: Vec<&TorrentFileEntry> = files
                .iter()
                .filter(|f| matches!(f.file_type, FileType::Video))
                .collect();
            let subs: Vec<&TorrentFileEntry> = files
                .iter()
                .filter(|f| matches!(f.file_type, FileType::Subtitle))
                .collect();
            let other: Vec<&TorrentFileEntry> = files
                .iter()
                .filter(|f| matches!(f.file_type, FileType::Other))
                .collect();

            if !videos.is_empty() {
                sections += 1;
                items.push(ListItem::new(Line::from(Span::styled(
                    format!("Video ({})", videos.len()),
                    Style::default().fg(accent).add_modifier(Modifier::BOLD),
                ))));
                for f in &videos {
                    items.push(file_list_item(f, Color::White));
                }
            }

            if !subs.is_empty() {
                sections += 1;
                items.push(ListItem::new(Line::from(Span::styled(
                    format!("Subtitles ({})", subs.len()),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                ))));
                for f in &subs {
                    items.push(file_list_item(f, Color::DarkGray));
                }
            }

            if !other.is_empty() {
                sections += 1;
                items.push(ListItem::new(Line::from(Span::styled(
                    format!("Other ({})", other.len()),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                ))));
                for f in &other {
                    items.push(file_list_item(f, Color::DarkGray));
                }
            }

            (items, sections)
        }
    }
}

fn file_list_item<'a>(entry: &TorrentFileEntry, color: Color) -> ListItem<'a> {
    // Show just the filename (last path component) to save space
    let name = entry
        .path
        .rsplit('/')
        .next()
        .unwrap_or(&entry.path)
        .to_string();
    let size = format_size(entry.size, BINARY);

    ListItem::new(Line::from(vec![
        Span::styled(format!("  {}", name), Style::default().fg(color)),
        Span::styled(format!("  {}", size), Style::default().fg(Color::DarkGray)),
    ]))
}

fn render_file_list(
    frame: &mut Frame,
    area: Rect,
    scroll_state: &mut ListState,
    items: &[ListItem],
    accent: Color,
) {
    let list =
        List::new(items.to_vec()).highlight_style(Style::default().bg(accent).fg(Color::Black));
    frame.render_stateful_widget(list, area, scroll_state);
}

fn render_mal_footer(frame: &mut Frame, area: Rect, preview: &PreviewState) {
    let line = match &preview.mal_info {
        PreviewSection::Loading => Line::from(Span::styled(
            "\u{25CB} Loading MAL data...",
            Style::default().fg(Color::DarkGray),
        )),
        PreviewSection::Error(msg) => Line::from(Span::styled(
            msg.clone(),
            Style::default().fg(Color::DarkGray),
        )),
        PreviewSection::Loaded(metadata) => {
            let (badge_text, badge_color) = match metadata.status.as_str() {
                "currently_airing" => ("[Airing]", Color::Green),
                "finished_airing" => ("[Finished]", Color::Blue),
                "not_yet_aired" => ("[Not Yet Aired]", Color::Yellow),
                _ => ("[Unknown]", Color::DarkGray),
            };

            let episodes = match metadata.episodes {
                Some(n) => format!("  Episodes: {}  ", n),
                None => "  Episodes: ?  ".to_string(),
            };

            let title_display = if metadata.title.len() > 30 {
                format!("{}...", &metadata.title[..27])
            } else {
                metadata.title.clone()
            };

            Line::from(vec![
                Span::styled(badge_text, Style::default().fg(badge_color)),
                Span::styled(episodes, Style::default().fg(Color::White)),
                Span::styled(title_display, Style::default().fg(Color::DarkGray)),
            ])
        }
    };

    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}

fn render_summary_line(frame: &mut Frame, area: Rect, preview: &PreviewState) {
    let text = match &preview.torrent_files {
        PreviewSection::Loaded(files) => {
            let total = files.len();
            let video_count = files
                .iter()
                .filter(|f| matches!(f.file_type, FileType::Video))
                .count();
            let total_size: u64 = files.iter().map(|f| f.size).sum();
            format!(
                "{} files | {} video | {}",
                total,
                video_count,
                format_size(total_size, BINARY)
            )
        }
        _ => "---".to_string(),
    };

    let paragraph = Paragraph::new(text).style(Style::default().fg(Color::DarkGray));
    frame.render_widget(paragraph, area);
}

pub fn render_glossary_popup(frame: &mut Frame, accent: Color) {
    let area = frame.area();

    let popup_height = 26u16.min(area.height.saturating_sub(4));
    let popup_width = 60u16.min(area.width.saturating_sub(6));

    let [popup_area] = Layout::horizontal([Constraint::Length(popup_width)])
        .flex(Flex::Center)
        .areas(area);
    let [popup_area] = Layout::vertical([Constraint::Length(popup_height)])
        .flex(Flex::Center)
        .areas(popup_area);

    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .title(" Torrent Terminology ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(accent));

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let text = vec![
        Line::from(Span::styled(
            "CODECS",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("  x264 (H.264)  ", Style::default().fg(Color::White)),
            Span::styled(
                "Older, larger files, universal support",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled("  x265 (HEVC)   ", Style::default().fg(Color::White)),
            Span::styled(
                "~50% smaller, same quality, needs more CPU",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled("  AV1           ", Style::default().fg(Color::White)),
            Span::styled(
                "Newest, smallest, slowest to decode",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "RESOLUTION",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("  1080p         ", Style::default().fg(Color::White)),
            Span::styled("Full HD (1920x1080)", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(vec![
            Span::styled("  720p          ", Style::default().fg(Color::White)),
            Span::styled(
                "HD (1280x720), smaller files",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled("  2160p / 4K    ", Style::default().fg(Color::White)),
            Span::styled(
                "Ultra HD, very large files",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "QUALITY",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("  10-bit        ", Style::default().fg(Color::White)),
            Span::styled(
                "Better gradients, slightly smaller",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled("  FLAC          ", Style::default().fg(Color::White)),
            Span::styled(
                "Lossless audio (bigger files)",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "RELEASE TYPES",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("  [BATCH]       ", Style::default().fg(Color::LightYellow)),
            Span::styled(
                "All episodes in one torrent",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled("  BD / Blu-ray  ", Style::default().fg(Color::White)),
            Span::styled(
                "From disc, best quality",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled("  WEB           ", Style::default().fg(Color::White)),
            Span::styled(
                "Streaming rip (Crunchyroll, etc)",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "INDICATORS",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("  ★             ", Style::default().fg(Color::LightGreen)),
            Span::styled(
                "Trusted uploader (Nyaa verified)",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Press Esc to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let paragraph = Paragraph::new(text);
    frame.render_widget(paragraph, inner);
}
