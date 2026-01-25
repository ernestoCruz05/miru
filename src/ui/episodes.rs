use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState},
    Frame,
};

use crate::library::Show;

use super::widgets::{format_episode_num, titled_block};

pub fn render_episodes_view(
    frame: &mut Frame,
    area: Rect,
    show: &Show,
    list_state: &mut ListState,
    accent: Color,
) {
    let items: Vec<ListItem> = show
        .episodes
        .iter()
        .map(|ep| {
            let status_icon = if ep.watched { "✓" } else { "○" };
            let status_color = if ep.watched {
                Color::Green
            } else {
                Color::DarkGray
            };

            let mut spans = vec![
                Span::styled(status_icon, Style::default().fg(status_color)),
                Span::raw(" "),
                Span::styled(
                    format_episode_num(ep.number),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(" - "),
                Span::raw(&ep.filename),
            ];

            // Show resume indicator if there's a saved position
            if ep.last_position > 0 && !ep.watched {
                let mins = ep.last_position / 60;
                let secs = ep.last_position % 60;
                spans.push(Span::styled(
                    format!(" ({}:{:02})", mins, secs),
                    Style::default().fg(Color::Yellow),
                ));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let title = format!("{} - Episodes", show.title);
    let list = List::new(items)
        .block(titled_block(&title, accent))
        .highlight_style(
            Style::default()
                .bg(accent)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, area, list_state);
}
