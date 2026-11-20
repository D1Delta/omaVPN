use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table, Tabs, Wrap};
use ratatui::Frame;

use crate::tui::app::{App, InputMode, Tab};

pub fn draw(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header / status bar
            Constraint::Length(3), // Tabs
            Constraint::Min(5),    // Body
            Constraint::Length(2), // Footer / keyhints + status message
        ])
        .split(frame.area());

    draw_header(frame, app, chunks[0]);
    draw_tabs(frame, app, chunks[1]);

    match app.tab {
        Tab::Servers => draw_servers(frame, app, chunks[2]),
        Tab::Subscriptions => draw_subscriptions(frame, app, chunks[2]),
        Tab::Logs => draw_logs(frame, app, chunks[2]),
        Tab::Settings => draw_settings(frame, app, chunks[2]),
    }

    draw_footer(frame, app, chunks[3]);

    if app.input_mode != InputMode::Normal {
        draw_input_popup(frame, app);
    }
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let (badge, badge_color) = if !app.daemon_reachable {
        (" DAEMON OFFLINE ", Color::Red)
    } else if app.daemon_status.running {
        (" CONNECTED ", Color::Green)
    } else {
        (" DISCONNECTED ", Color::Yellow)
    };

    let active_name = app
        .daemon_status
        .active_profile_name
        .as_deref()
        .unwrap_or("none");

    let engine_display = if let Some(ref active_eng) = app.daemon_status.engine {
        format!("{} ({active_eng})", app.settings.engine.as_str())
    } else {
        app.settings.engine.as_str().to_string()
    };

    let line = Line::from(vec![
        Span::styled(
            " omavpn ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            badge,
            Style::default()
                .fg(Color::Black)
                .bg(badge_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  Server: "),
        Span::styled(active_name, Style::default().add_modifier(Modifier::BOLD)),
        Span::raw("  |  Core: "),
        Span::styled(engine_display, Style::default().fg(Color::Green)),
        Span::raw("  |  Mode: "),
        Span::styled(app.settings.mode.as_str(), Style::default().fg(Color::Cyan)),
        Span::raw("  |  Route: "),
        Span::styled(
            app.settings.routing.as_str(),
            Style::default().fg(Color::Magenta),
        ),
    ]);

    let header = Paragraph::new(line)
        .block(Block::default().borders(Borders::ALL).title("Status"));
    frame.render_widget(header, area);
}

fn draw_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = Tab::ALL.iter().map(|t| Line::from(t.title())).collect();
    let idx = Tab::ALL.iter().position(|t| *t == app.tab).unwrap_or(0);
    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL))
        .select(idx)
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(tabs, area);
}

fn draw_servers(frame: &mut Frame, app: &App, area: Rect) {
    let filtered = app.filtered_profiles();
    let active_id = app.daemon_status.active_profile_id.as_deref();

    let header = Row::new(vec!["", "Name", "Proto", "Endpoint", "Ping"])
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .bottom_margin(1);

    let rows: Vec<Row> = filtered
        .iter()
        .enumerate()
        .map(|(row_idx, (_, p))| {
            let is_active = app.daemon_status.running && active_id == Some(p.id.as_str());
            let marker = if is_active { "●" } else { " " };
            let latency_cell = match p.latency_ms {
                Some(ms) if ms >= 0 => {
                    let color = if ms < 150 {
                        Color::Green
                    } else if ms < 350 {
                        Color::Yellow
                    } else {
                        Color::Red
                    };
                    Cell::from(format!("{ms} ms")).style(Style::default().fg(color))
                }
                Some(_) => Cell::from("timeout").style(Style::default().fg(Color::Red)),
                None => Cell::from("—").style(Style::default().fg(Color::DarkGray)),
            };

            let style = if row_idx == app.selected_server {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else if is_active {
                Style::default().fg(Color::Green)
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(marker),
                Cell::from(p.display_name().to_string()),
                Cell::from(p.protocol.as_str()),
                Cell::from(format!("{}:{}", p.server, p.port)),
                latency_cell,
            ])
            .style(style)
        })
        .collect();

    let title = if app.search_query.is_empty() {
        format!("Servers ({})", filtered.len())
    } else {
        format!("Servers ({}) [filter: {}]", filtered.len(), app.search_query)
    };

    let widths = [
        Constraint::Length(2),
        Constraint::Percentage(42),
        Constraint::Length(8),
        Constraint::Percentage(34),
        Constraint::Length(10),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(title));

    frame.render_widget(table, area);
}

fn draw_subscriptions(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = if app.subscriptions.is_empty() {
        vec![ListItem::new(
            "No subscriptions yet. Press 'a' to add a subscription URL.",
        )]
    } else {
        app.subscriptions
            .iter()
            .enumerate()
            .map(|(i, sub)| {
                let updated = sub
                    .last_updated
                    .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "never".to_string());
                let style = if i == app.selected_sub {
                    Style::default()
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(format!("{}  |  {}  (updated: {})", sub.name, sub.url, updated))
                    .style(style)
            })
            .collect()
    };

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Subscriptions ({})", app.subscriptions.len())),
    );
    frame.render_widget(list, area);
}

fn draw_logs(frame: &mut Frame, app: &App, area: Rect) {
    let text = if app.logs.is_empty() {
        "No logs from omavpnd yet. Connect to a server or press 'u' to refresh.".to_string()
    } else {
        let max_start = app.logs.len().saturating_sub(1);
        let start = app.log_scroll.min(max_start);
        app.logs[start..].join("\n")
    };

    let paragraph = Paragraph::new(text)
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title("sing-box Logs"));
    frame.render_widget(paragraph, area);
}

fn draw_settings(frame: &mut Frame, app: &App, area: Rect) {
    let rows = [
        format!("Run Mode          : {}", app.settings.mode.as_str()),
        format!("Core Engine       : {} (press 'c' to cycle)", app.settings.engine.as_str()),
        format!("Routing Preset    : {}", app.settings.routing.as_str()),
        format!(
            "IPv6 Enabled      : {}",
            if app.settings.ipv6_enabled { "yes" } else { "no" }
        ),
        format!("TUN Interface     : {} (MTU {})", app.settings.tun_interface, app.settings.tun_mtu),
        format!("Local Mixed Proxy : 127.0.0.1:{}", app.settings.mixed_port),
        format!("Remote DNS        : {}", app.settings.remote_dns),
        format!("Local DNS         : {}", app.settings.local_dns),
        format!("Config Directory  : {}", app.storage.base_dir().display()),
    ];

    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(i, text)| {
            let style = if i == app.selected_setting {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(text.as_str()).style(style)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Settings (Enter to toggle highlighted option)"),
    );
    frame.render_widget(list, area);
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let hints = match app.tab {
        Tab::Servers => {
            "Enter: connect | Space: toggle | a: paste link/sub | i: clipboard | p: ping | s: sort | /: filter | c: core | m: mode | r: route | q: quit"
        }
        Tab::Subscriptions => "a: add sub | u/Enter: update all | d: del | Tab: switch | q: quit",
        Tab::Logs => "j/k: scroll | g/G: top/bottom | u: refresh | Tab: switch | q: quit",
        Tab::Settings => "j/k: select | Enter: toggle | Tab: switch | q: quit",
    };

    let text = vec![
        Line::from(Span::styled(
            app.status_message.as_str(),
            Style::default().fg(Color::Yellow),
        )),
        Line::from(Span::styled(hints, Style::default().fg(Color::DarkGray))),
    ];
    frame.render_widget(Paragraph::new(text), area);
}

fn draw_input_popup(frame: &mut Frame, app: &App) {
    let (title, height) = match &app.input_mode {
        InputMode::Search => ("Filter servers (Enter to apply, Esc to cancel)".to_string(), 5),
        InputMode::AddLink => (
            "Paste VPN link (vless://, vmess://, hy2://...) or Sub URL (https://...) — Enter: add, Esc: cancel".to_string(),
            7,
        ),
        InputMode::AddSubName => ("New Subscription — Name (Enter: next, Esc: cancel)".to_string(), 5),
        InputMode::AddSubUrl { name } => (
            format!("Subscription '{name}' — URL (Ctrl+Shift+V / Ctrl+V to paste, Enter: save)"),
            7,
        ),
        InputMode::Normal => return,
    };

    let popup = centered_rect(78, height, frame.area());
    frame.render_widget(Clear, popup);
    let input = Paragraph::new(app.input_buffer.as_str())
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(title),
        );
    frame.render_widget(input, popup);
}

fn centered_rect(percent_x: u16, height: u16, r: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(height),
            Constraint::Min(1),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}
