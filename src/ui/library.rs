use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::library::Show;

use super::widgets::titled_block;

pub fn render_library_view(
    frame: &mut Frame,
    area: Rect,
    shows: &[Show],
    list_state: &mut ListState,
    accent: Color,
) {
    let items: Vec<ListItem> = shows
        .iter()
        .map(|show| {
            let watched = show.watched_count();
            let total = show.episode_count();

            let progress = format!("{}/{}", watched, total);
            let progress_color = if watched == total && total > 0 {
                Color::Green
            } else if watched > 0 {
                Color::Yellow
            } else {
                Color::DarkGray
            };

            let next_info = if let Some(next) = show.next_unwatched() {
                Span::styled(
                    format!("  Next: Ep {}", next.number),
                    Style::default().fg(Color::Cyan),
                )
            } else {
                Span::raw("")
            };

            let line = Line::from(vec![
                Span::raw(&show.title),
                Span::raw(" "),
                Span::styled(progress, Style::default().fg(progress_color)),
                next_info,
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(titled_block("Library", accent))
        .highlight_style(
            Style::default()
                .bg(accent)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let list_area = layout[0];
    let details_area = layout[1];

    frame.render_stateful_widget(list, list_area, list_state);

    // Render Details
    render_details(frame, details_area, shows, list_state, accent);
}

fn render_details(
    frame: &mut Frame,
    area: Rect,
    shows: &[Show],
    list_state: &ListState,
    accent: Color,
) {
    let block = Block::default()
        .title(" Details ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(accent));
    
    let inner_area = block.inner(area);
    frame.render_widget(block, area);

    if let Some(idx) = list_state.selected() {
        if let Some(show) = shows.get(idx) {
            // Layout: Top for Title/Info, Bottom for Synopsis
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1), // Title
                    Constraint::Length(1), // Gap
                    Constraint::Length(4), // Meta info
                    Constraint::Length(1), // Gap
                    Constraint::Min(0),    // Synopsis
                ])
                .split(inner_area);

            // Title
            let title = &show.title;
            frame.render_widget(
                Paragraph::new(Span::styled(title, Style::default().add_modifier(Modifier::BOLD).fg(accent))),
                chunks[0]
            );

            // Metadata info
            let mut info_text = Vec::new();
            if let Some(meta) = &show.metadata {
                info_text.push(Line::from(vec![
                    Span::styled("Score: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(meta.score.map(|s| format!("{:.2}", s)).unwrap_or_else(|| "N/A".to_string())),
                ]));
                info_text.push(Line::from(vec![
                    Span::styled("Status: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(&meta.status),
                ]));
                if let Some(episodes) = meta.episodes {
                    info_text.push(Line::from(vec![
                        Span::styled("Episodes: ", Style::default().fg(Color::DarkGray)),
                        Span::raw(episodes.to_string()),
                    ]));
                }
                 info_text.push(Line::from(vec![
                    Span::styled("Genres: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(meta.genres.join(", ")),
                ]));
            } else {
                info_text.push(Line::from(Span::styled("No metadata available.", Style::default().fg(Color::DarkGray))));
                info_text.push(Line::from(""));
                info_text.push(Line::from(Span::styled("Press 'm' to fetch metadata", Style::default().fg(Color::DarkGray))));
            }
            
            frame.render_widget(Paragraph::new(info_text), chunks[2]);

            // Synopsis
            if let Some(meta) = &show.metadata {
                if let Some(synopsis) = &meta.synopsis {
                    frame.render_widget(
                        Paragraph::new(synopsis.as_str())
                            .wrap(Wrap { trim: true })
                            .block(Block::default().borders(Borders::TOP).title(" Synopsis ")),
                        chunks[4]
                    );
                }
            }
        }
    } else {
        frame.render_widget(
            Paragraph::new("Select a show to view details")
                .alignment(ratatui::layout::Alignment::Center)
                .style(Style::default().fg(Color::DarkGray)),
            inner_area
        );
    }
}
