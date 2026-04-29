use crate::app::App;
use crate::theme::Theme;
use crate::token_monitor::{AuthField, AuthMode};
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;

pub(crate) fn draw_token_monitor_overlay(f: &mut Frame, app: &App, theme: &Theme) {
    let area = f.area();

    let popup_w = 76u16.min(area.width.saturating_sub(4));
    let popup_h = 22u16.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(popup_w)) / 2;
    let y = (area.height.saturating_sub(popup_h)) / 2;
    let popup = Rect::new(x, y, popup_w, popup_h);

    f.render_widget(Clear, popup);

    let block = Block::default()
        .style(Style::default().bg(theme.main_bg))
        .title(
            Line::from(vec![Span::styled(
                " Tokens-Monitor ",
                Style::default()
                    .fg(theme.title)
                    .add_modifier(Modifier::BOLD),
            )])
            .alignment(Alignment::Center),
        )
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.cpu_box));
    f.render_widget(block, popup);

    let inner = Rect::new(
        popup.x + 2,
        popup.y + 1,
        popup.width.saturating_sub(4),
        popup.height.saturating_sub(2),
    );

    let form = &app.token_monitor_form;
    let selected = form.selected_field();
    let client = &app.token_monitor_client;
    let status = client.status();
    let enabled = client.config().enabled;

    let mut lines = vec![
        Line::from(vec![
            Span::styled(" 认证模式 ", Style::default().fg(theme.graph_text)),
            Span::styled(
                form.mode.label(),
                Style::default()
                    .fg(theme.title)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  (F2 切换登录/注册)",
                Style::default().fg(theme.inactive_fg),
            ),
        ]),
        Line::from(vec![
            Span::styled(" 当前身份 ", Style::default().fg(theme.graph_text)),
            Span::styled(client.auth_label(), Style::default().fg(theme.main_fg)),
        ]),
        Line::from(vec![
            Span::styled(" 上报状态 ", Style::default().fg(theme.graph_text)),
            Span::styled(
                if enabled { "已启用" } else { "已暂停" },
                Style::default()
                    .fg(if enabled {
                        theme.proc_misc
                    } else {
                        theme.warning_fg
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "  queue={} sent={} failed={}",
                    status.queue_len, status.total_sent, status.total_failed
                ),
                Style::default().fg(theme.main_fg),
            ),
        ]),
    ];

    if !status.last_ok_at.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(" 最近成功 ", Style::default().fg(theme.graph_text)),
            Span::styled(&status.last_ok_at, Style::default().fg(theme.main_fg)),
        ]));
    }
    if !status.last_error.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(" 最近错误 ", Style::default().fg(theme.warning_fg)),
            Span::styled(&status.last_error, Style::default().fg(theme.warning_fg)),
        ]));
    }

    lines.push(Line::from(""));
    lines.extend(field_lines(app, selected, theme));
    lines.push(Line::from(""));

    // Stats line: total records / total tokens, and last report result
    let tokens_str = format_tokens(status.total_tokens_sent);
    let stats_text = if !status.last_error.is_empty() {
        format!(
            " 已上报 {} 条 / {} tokens  |  最近: ✗ {}",
            status.total_sent, tokens_str, status.last_error
        )
    } else if status.total_sent > 0 {
        let last_ok_short = if status.last_ok_at.len() > 19 {
            &status.last_ok_at[..19]
        } else {
            &status.last_ok_at
        };
        format!(
            " 已上报 {} 条 / {} tokens  |  最近: ✓ {}",
            status.total_sent, tokens_str, last_ok_short
        )
    } else {
        format!(" 暂无上报记录")
    };
    lines.push(Line::from(Span::styled(
        stats_text,
        Style::default().fg(theme.inactive_fg),
    )));

    if !form.message.is_empty() {
        lines.push(Line::from(Span::styled(
            format!(" {}", form.message),
            Style::default().fg(theme.status_fg),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            " Enter 执行动作，Ctrl+S 提交认证，Esc 关闭 ",
            Style::default().fg(theme.inactive_fg),
        )));
    }

    f.render_widget(Paragraph::new(lines), inner);
}

fn field_lines(app: &App, selected: AuthField, theme: &Theme) -> Vec<Line<'static>> {
    let form = &app.token_monitor_form;
    let enabled = app.token_monitor_client.config().enabled;
    let mut lines = Vec::new();
    let masked_password = if form.password.is_empty() {
        String::new()
    } else {
        "*".repeat(form.password.chars().count())
    };

    lines.push(field_line(
        AuthField::ServerUrl,
        selected,
        "Server URL",
        &form.server_url,
        theme,
    ));
    lines.push(field_line(
        AuthField::Email,
        selected,
        "Email / User ID",
        &form.email,
        theme,
    ));
    lines.push(field_line(
        AuthField::Password,
        selected,
        "Password",
        &masked_password,
        theme,
    ));

    if form.mode == AuthMode::Register {
        lines.push(field_line(
            AuthField::Name,
            selected,
            "Name",
            &form.name,
            theme,
        ));
        lines.push(field_line(
            AuthField::Department,
            selected,
            "Department",
            &form.department,
            theme,
        ));
    }

    lines.push(button_line(
        AuthField::Submit,
        selected,
        &format!("{}并保存", form.mode.label()),
        theme,
    ));
    lines.push(button_line(
        AuthField::Enable,
        selected,
        if enabled {
            "暂停上报"
        } else {
            "启用上报"
        },
        theme,
    ));
    lines.push(button_line(
        AuthField::ClearAuth,
        selected,
        "清除登录态",
        theme,
    ));
    lines
}

fn field_line(
    field: AuthField,
    selected: AuthField,
    label: &str,
    value: &str,
    theme: &Theme,
) -> Line<'static> {
    let is_selected = field == selected;
    let label_style = if is_selected {
        Style::default()
            .fg(theme.selected_fg)
            .bg(theme.selected_bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.main_fg)
    };
    let value_style = if is_selected {
        Style::default().fg(theme.selected_fg).bg(theme.selected_bg)
    } else {
        Style::default().fg(theme.hi_fg)
    };
    let cursor = if is_selected { ">" } else { " " };
    let value = if value.is_empty() { " " } else { value };
    Line::from(vec![
        Span::styled(format!("{cursor} {:<16}", label), label_style),
        Span::styled(value.to_string(), value_style),
    ])
}

fn button_line(field: AuthField, selected: AuthField, label: &str, theme: &Theme) -> Line<'static> {
    let is_selected = field == selected;
    let style = if is_selected {
        Style::default()
            .fg(theme.selected_fg)
            .bg(theme.selected_bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.proc_misc)
    };
    let cursor = if is_selected { ">" } else { " " };
    Line::from(Span::styled(format!("{cursor} [{label}]"), style))
}

fn format_tokens(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.1}B", n as f64 / 1_000_000_000.0)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
