use crate::app::App;
use crate::theme::Theme;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;

pub(crate) fn draw_config_overlay(f: &mut Frame, app: &App, theme: &Theme) {
    let area = f.area();

    let popup_w = 50u16.min(area.width.saturating_sub(4));
    let popup_h = 14u16.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(popup_w)) / 2;
    let y = (area.height.saturating_sub(popup_h)) / 2;
    let popup = Rect::new(x, y, popup_w, popup_h);

    f.render_widget(Clear, popup);

    let block = Block::default()
        .style(Style::default().bg(theme.main_bg))
        .title(Line::from(vec![
            Span::styled(
                " Config ",
                Style::default().fg(theme.title).add_modifier(Modifier::BOLD),
            ),
        ]).alignment(Alignment::Center))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.cpu_box));
    f.render_widget(block, popup);

    let inner = Rect::new(popup.x + 2, popup.y + 1, popup.width.saturating_sub(4), popup.height.saturating_sub(2));

    let items: Vec<(&str, String)> = vec![
        ("Theme", app.theme.name.to_string()),
        ("Context panel (1)", toggle_str(app.show_context)),
        ("Quota panel (2)", toggle_str(app.show_quota)),
        ("Tokens panel (3)", toggle_str(app.show_tokens)),
        ("Ports panel (4)", toggle_str(app.show_ports)),
        ("Sessions panel (5)", toggle_str(app.show_sessions)),
    ];

    let mut lines = Vec::new();
    lines.push(Line::from(""));

    for (i, (label, value)) in items.iter().enumerate() {
        let selected = i == app.config_selected;
        let cursor = if selected { ">" } else { " " };

        let label_style = if selected {
            Style::default().fg(theme.selected_fg).bg(theme.selected_bg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.main_fg)
        };

        let value_style = if selected {
            Style::default().fg(theme.selected_fg).bg(theme.selected_bg)
        } else if value == "on" {
            Style::default().fg(theme.proc_misc)
        } else if value == "off" {
            Style::default().fg(theme.inactive_fg)
        } else {
            Style::default().fg(theme.session_id)
        };

        let label_w = 22;
        let padded_label = format!("{} {:<width$}", cursor, label, width = label_w);
        let padded_value = format!("{:<10}", value);

        lines.push(Line::from(vec![
            Span::styled(padded_label, label_style),
            Span::styled(padded_value, value_style),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!(" abtop v{}  Enter/Space to change  Esc to close", env!("CARGO_PKG_VERSION")),
        Style::default().fg(theme.graph_text),
    )));

    f.render_widget(Paragraph::new(lines), inner);
}

fn toggle_str(v: bool) -> String {
    if v { "on".into() } else { "off".into() }
}
