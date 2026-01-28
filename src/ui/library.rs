use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use ratatui_image::picker::Picker;
use ratatui_image::{Resize, Image};
use crate::image_cache::ImageCache;

use crate::library::Show;

use super::widgets::titled_block;

pub fn render_library_view(
    frame: &mut Frame,
    area: Rect,
    shows: &[Show],
    list_state: &mut ListState,
    accent: Color,
    image_cache: &ImageCache,
    picker: &mut Picker,
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
    render_details(frame, details_area, shows, list_state, accent, image_cache, picker);
}

fn render_details(
    frame: &mut Frame,
    area: Rect,
    shows: &[Show],
    list_state: &ListState,
    accent: Color,
    image_cache: &ImageCache,
    picker: &mut Picker,
) {
    let block = Block::default()
        .title(" Details ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(accent));
    
    let inner_area = block.inner(area);
    frame.render_widget(block, area);

    if let Some(idx) = list_state.selected() {
        if let Some(show) = shows.get(idx) {
            
            // Split into Text (Left) and Image (Right)
            // But if image is not avaialble, maybe take full width?
            // Actually consistency is good.
            
            let content_layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(0), Constraint::Length(30)])
                .split(inner_area);
                
            let text_area = content_layout[0];
            let image_area = content_layout[1];

            // Layout for Text: Top for Title/Info, Bottom for Synopsis
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1), // Title
                    Constraint::Length(1), // Gap
                    Constraint::Length(4), // Meta info
                    Constraint::Length(1), // Gap
                    Constraint::Min(0),    // Synopsis
                ])
                .split(text_area);

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
                
                // Render Image
                if let Some(url) = &meta.cover_url {
                     if let Some(img) = image_cache.get(url) {
                        if let Ok(protocol) = picker.new_protocol(img, image_area, Resize::Fit(None)) {
                             let widget = Image::new(&protocol);
                             frame.render_widget(widget, image_area);
                        }
                     }
                }
                     // Else we could trigger download here if not present, but App handles logic.
                     // App doesn't know *which* show is visible unless we tell it.
                     // But we only fetch metadata for *selected*.
                     // App logic: When MetadataFound, download cover.
                     // On load (start), if metadata exists, we might need to check if cover is cached?
                     // If it's cached, we are good.
                     // If not, we might want to trigger download.
                // For now, assume cached or downloaded on fetch.

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
