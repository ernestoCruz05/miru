use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

/// Create a styled block with a title
pub fn titled_block(title: &str, accent: Color) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(accent))
        .title(format!(" {} ", title))
        .title_style(Style::default().fg(accent).add_modifier(Modifier::BOLD))
}

/// Create a help bar at the bottom
pub fn help_bar<'a>(hints: &'a [(&'a str, &'a str)]) -> Paragraph<'a> {
    let spans: Vec<Span> = hints
        .iter()
        .enumerate()
        .flat_map(|(i, (key, action))| {
            let mut v = vec![
                Span::styled(*key, Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" "),
                Span::styled(*action, Style::default().fg(Color::DarkGray)),
            ];
            if i < hints.len() - 1 {
                v.push(Span::raw("  "));
            }
            v
        })
        .collect();

    Paragraph::new(Line::from(spans))
}

/// Format episode number with leading zeros
pub fn format_episode_num(num: u32) -> String {
    format!("{:02}", num)
}

/// Parse accent color from config string
pub fn parse_accent_color(color: &str) -> Color {
    match color.to_lowercase().as_str() {
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "gray" | "grey" => Color::Gray,
        _ => Color::Magenta, // default
    }
}
