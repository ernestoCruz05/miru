use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState},
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

            let line = Line::from(vec![
                Span::raw(&show.title),
                Span::raw(" "),
                Span::styled(progress, Style::default().fg(progress_color)),
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

    frame.render_stateful_widget(list, area, list_state);
}
