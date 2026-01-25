use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::nyaa::{NyaaCategory, NyaaFilter, NyaaResult};

use super::widgets::titled_block;

pub fn render_search_view(
    frame: &mut Frame,
    area: Rect,
    query: &str,
    results: &[NyaaResult],
    list_state: &mut ListState,
    is_loading: bool,
    category: NyaaCategory,
    filter: NyaaFilter,
    accent: Color,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Search input
            Constraint::Length(1), // Filter bar
            Constraint::Min(3),    // Results
        ])
        .split(area);

    // Search input
    render_search_input(frame, chunks[0], query, is_loading, accent);

    // Filter bar
    render_filter_bar(frame, chunks[1], category, filter);

    // Results list
    render_search_results(frame, chunks[2], results, list_state, accent);
}

fn render_search_input(frame: &mut Frame, area: Rect, query: &str, is_loading: bool, accent: Color) {
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

    // Show cursor at end of input
    frame.set_cursor_position((area.x + query.len() as u16 + 1, area.y + 1));
}

fn render_filter_bar(frame: &mut Frame, area: Rect, category: NyaaCategory, filter: NyaaFilter) {
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

    let items: Vec<ListItem> = results
        .iter()
        .map(|r| {
            // Color seeders based on health
            let seeder_color = if r.seeders >= 50 {
                Color::Green
            } else if r.seeders >= 10 {
                Color::Yellow
            } else if r.seeders > 0 {
                Color::Red
            } else {
                Color::DarkGray
            };

            let line = Line::from(vec![
                Span::styled(
                    format!("{:>4}", r.seeders),
                    Style::default().fg(seeder_color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" │ "),
                Span::styled(
                    format!("{:>9}", r.size),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw(" │ "),
                Span::raw(&r.title),
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
