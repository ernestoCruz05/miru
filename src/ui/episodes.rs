use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState},
};

use crate::library::{Show, models::Episode};

use super::widgets::{format_episode_num, titled_block};

fn episode_list_item(ep: &Episode, indent: &str) -> ListItem<'static> {
    let status_icon = if ep.watched { "✓" } else { "○" };
    let status_color = if ep.watched {
        Color::Green
    } else {
        Color::DarkGray
    };

    let mut spans = vec![
        Span::raw(indent.to_string()),
        Span::styled(status_icon.to_string(), Style::default().fg(status_color)),
        Span::raw(" "),
        Span::styled(
            format_episode_num(ep.number),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(" - "),
        Span::raw(ep.filename.clone()),
    ];

    if ep.last_position > 0 && !ep.watched {
        let mins = ep.last_position / 60;
        let secs = ep.last_position % 60;
        spans.push(Span::styled(
            format!(" ({}:{:02})", mins, secs),
            Style::default().fg(Color::Yellow),
        ));
    }

    ListItem::new(Line::from(spans))
}

pub fn render_episodes_view(
    frame: &mut Frame,
    area: Rect,
    show: &Show,
    list_state: &mut ListState,
    accent: Color,
) {
    let mut items: Vec<ListItem> = Vec::new();

    if show.is_seasonal() {
        for season in &show.seasons {
            let watched = season.episodes.iter().filter(|e| e.watched).count();
            let total = season.episodes.len();
            let progress_color = if watched == total && total > 0 {
                Color::Green
            } else if watched > 0 {
                Color::Yellow
            } else {
                Color::DarkGray
            };

            items.push(ListItem::new(Line::from(vec![
                Span::styled(
                    format!("▸ Season {} ", season.number),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("({}/{})", watched, total),
                    Style::default().fg(progress_color),
                ),
            ])));

            for ep in &season.episodes {
                items.push(episode_list_item(ep, "  "));
            }
        }

        if !show.specials.is_empty() {
            items.push(ListItem::new(Line::from(vec![
                Span::styled(
                    "▸ Specials ",
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("({})", show.specials.len()),
                    Style::default().fg(Color::DarkGray),
                ),
            ])));

            for ep in &show.specials {
                items.push(episode_list_item(ep, "  "));
            }
        }

        if !show.episodes.is_empty() {
            items.push(ListItem::new(Line::from(vec![
                Span::styled(
                    "▸ Episodes ",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("({})", show.episodes.len()),
                    Style::default().fg(Color::DarkGray),
                ),
            ])));

            for ep in &show.episodes {
                items.push(episode_list_item(ep, "  "));
            }
        }
    } else {
        for ep in &show.episodes {
            items.push(episode_list_item(ep, ""));
        }
    }

    let title = if show.is_seasonal() {
        format!("{} - {} Seasons", show.title, show.seasons.len())
    } else {
        format!("{} - Episodes", show.title)
    };

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
