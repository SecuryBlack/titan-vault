use crate::tui::app::{App, Tab};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Tabs, Wrap},
    Frame,
};

pub fn render(frame: &mut Frame, app: &App) {
    let size = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header & Tabs
            Constraint::Min(10),   // Main Content Area
            Constraint::Length(3), // Status & Help bar
        ])
        .split(size);

    render_header(frame, chunks[0], app);
    render_content(frame, chunks[1], app);
    render_footer(frame, chunks[2], app);

    if app.live_backup_in_progress {
        render_live_backup_modal(frame, size, app);
    }
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let tab_titles: Vec<Line> = Tab::all()
        .iter()
        .map(|t| {
            let is_selected = *t == app.current_tab;
            let style = if is_selected {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            Line::from(Span::styled(t.title(), style))
        })
        .collect();

    let tabs = Tabs::new(tab_titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(
                    " 🛡️  TitanVault — SecuryBlack Standalone Backup Console ",
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                )),
        )
        .select(Tab::all().iter().position(|t| *t == app.current_tab).unwrap_or(0))
        .highlight_style(Style::default().fg(Color::Cyan));

    frame.render_widget(tabs, area);
}

fn render_content(frame: &mut Frame, area: Rect, app: &App) {
    match app.current_tab {
        Tab::Sources => render_sources_tab(frame, area, app),
        Tab::Targets => render_targets_tab(frame, area, app),
        Tab::PolicyCrypto => render_policy_crypto_tab(frame, area, app),
        Tab::Snapshots => render_snapshots_tab(frame, area, app),
    }
}

fn render_sources_tab(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // Bases de datos
    let mut db_lines = Vec::new();
    if app.config.sources.databases.is_empty() {
        db_lines.push(Line::from(Span::styled(
            "  No databases configured yet. (Add postgres/mysql in config.toml)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (i, db) in app.config.sources.databases.iter().enumerate() {
            let is_sel = app.selected_index == i;
            let status = if db.enabled { "● ACTIVE" } else { "○ DISABLED" };
            let style = if is_sel {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            db_lines.push(Line::from(vec![
                Span::styled(format!("  [{status}] "), Style::default().fg(if db.enabled { Color::Green } else { Color::Red })),
                Span::styled(format!("{} ({})", db.name, db.driver), style),
            ]));
            db_lines.push(Line::from(format!(
                "      DB: {} | Container: {}",
                db.database,
                db.container_name.as_deref().unwrap_or("none")
            )));
            db_lines.push(Line::raw(""));
        }
    }

    let dbs_block = Paragraph::new(db_lines)
        .block(Block::default().borders(Borders::ALL).title(" 🗄️ Database Sources "))
        .wrap(Wrap { trim: true });
    frame.render_widget(dbs_block, chunks[0]);

    // Directorios / Filesystems
    let mut fs_lines = Vec::new();
    if app.config.sources.filesystems.is_empty() {
        fs_lines.push(Line::from(Span::styled(
            "  No filesystem paths configured. (e.g. /opt/stack)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for fs in &app.config.sources.filesystems {
            let status = if fs.enabled { "● ACTIVE" } else { "○ DISABLED" };
            fs_lines.push(Line::from(vec![
                Span::styled(format!("  [{status}] "), Style::default().fg(if fs.enabled { Color::Green } else { Color::Red })),
                Span::styled(&fs.name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            ]));
            for p in &fs.paths {
                fs_lines.push(Line::from(format!("      Path: {}", p.display())));
            }
            if !fs.excludes.is_empty() {
                fs_lines.push(Line::from(format!("      Excludes: {}", fs.excludes.join(", "))));
            }
            fs_lines.push(Line::raw(""));
        }
    }

    let fs_block = Paragraph::new(fs_lines)
        .block(Block::default().borders(Borders::ALL).title(" 📁 Filesystem Sources "))
        .wrap(Wrap { trim: true });
    frame.render_widget(fs_block, chunks[1]);
}

fn render_targets_tab(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines = Vec::new();

    lines.push(Line::from(Span::styled(
        "Configured Multi-Cloud Storage Destinations (Powered by Apache OpenDAL):",
        Style::default().fg(Color::Cyan),
    )));
    lines.push(Line::raw(""));

    // 1. Hetzner Object Storage
    let hetzner_status = match &app.config.targets.hetzner {
        Some(h) if h.enabled => format!("[● ACTIVE] Bucket: {} ({})", h.bucket, h.endpoint),
        Some(_) => "[○ DISABLED] Configured but inactive".to_string(),
        None => "[○ NOT CONFIGURED] Hetzner S3".to_string(),
    };
    lines.push(Line::from(vec![
        Span::styled("1. Hetzner Object Storage: ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(hetzner_status, Style::default().fg(Color::Green)),
    ]));

    // 2. Cloudflare R2
    let r2_status = match &app.config.targets.cloudflare_r2 {
        Some(r) if r.enabled => format!("[● ACTIVE] Bucket: {} ({})", r.bucket, r.endpoint),
        Some(_) => "[○ DISABLED] Configured but inactive".to_string(),
        None => "[○ NOT CONFIGURED] Cloudflare R2 (S3 API)".to_string(),
    };
    lines.push(Line::from(vec![
        Span::styled("2. Cloudflare R2: ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(r2_status, Style::default().fg(Color::Green)),
    ]));

    // 3. Local Storage
    let local_status = match &app.config.targets.local {
        Some(l) if l.enabled => format!("[● ACTIVE] Path: {}", l.path.display()),
        Some(_) => "[○ DISABLED]".to_string(),
        None => "[○ NOT CONFIGURED] Local storage path".to_string(),
    };
    lines.push(Line::from(vec![
        Span::styled("3. Local Storage / NAS: ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(local_status, Style::default().fg(Color::Green)),
    ]));

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "Actions: Press [T] to Test Connectivity against active targets.",
        Style::default().fg(Color::Yellow),
    )));

    if let Some(res) = &app.test_result {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(format!("Connectivity Test Result: {res}"), Style::default().fg(Color::LightGreen))));
    }

    let targets_widget = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" ☁️ Storage Targets "))
        .wrap(Wrap { trim: true });
    frame.render_widget(targets_widget, area);
}

fn render_policy_crypto_tab(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // Retención GFS & Cron
    let ret = &app.config.retention;
    let sched = &app.config.schedule;
    let policy_lines = vec![
        Line::from(Span::styled("Grandfather-Father-Son (GFS) Retention Rules:", Style::default().fg(Color::Cyan))),
        Line::raw(""),
        Line::from(format!("  • Hourly copies to keep:  {}", ret.keep_hourly)),
        Line::from(format!("  • Daily copies to keep:   {}", ret.keep_daily)),
        Line::from(format!("  • Weekly copies to keep:  {}", ret.keep_weekly)),
        Line::from(format!("  • Monthly copies to keep: {}", ret.keep_monthly)),
        Line::from(format!("  • Yearly copies to keep:  {}", ret.keep_yearly)),
        Line::raw(""),
        Line::from(Span::styled("Scheduler Configuration:", Style::default().fg(Color::Cyan))),
        Line::from(format!("  • Autonomous Scheduler: {}", if sched.enabled { "ENABLED" } else { "DISABLED" })),
        Line::from(format!("  • Daily Schedule Cron:  {}", sched.cron)),
    ];

    let policy_block = Paragraph::new(policy_lines)
        .block(Block::default().borders(Borders::ALL).title(" ⏰ Retention & Schedule "))
        .wrap(Wrap { trim: true });
    frame.render_widget(policy_block, chunks[0]);

    // Cifrado Zero-Knowledge
    let crypto = &app.config.crypto;
    let crypto_lines = vec![
        Line::from(Span::styled("Client-Side Zero-Knowledge Encryption:", Style::default().fg(Color::Cyan))),
        Line::raw(""),
        Line::from(format!("  • Status:    {}", if crypto.enabled { "● ACTIVE (Payloads encrypted before upload)" } else { "○ DISABLED" })),
        Line::from(format!("  • Algorithm: {}", crypto.algorithm)),
        Line::from(format!("  • Key Source: {}", if crypto.passphrase.is_some() { "Direct Passphrase" } else if crypto.key_file.is_some() { "Master Key File" } else { "None" })),
        Line::raw(""),
        Line::from(Span::styled(
            "Note: Encrypted backups use ChaCha20-Poly1305 with random per-snapshot nonces.",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let crypto_block = Paragraph::new(crypto_lines)
        .block(Block::default().borders(Borders::ALL).title(" 🔒 Zero-Knowledge Security "))
        .wrap(Wrap { trim: true });
    frame.render_widget(crypto_block, chunks[1]);
}

fn render_snapshots_tab(frame: &mut Frame, area: Rect, app: &App) {
    let mut items = Vec::new();

    if app.snapshots.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "  No snapshots loaded. Press [L] to fetch remote snapshot catalog.",
            Style::default().fg(Color::DarkGray),
        ))));
    } else {
        for (i, snap) in app.snapshots.iter().enumerate() {
            let is_sel = app.selected_index == i;
            let style = if is_sel {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let size_mb = snap.size_bytes as f64 / (1024.0 * 1024.0);
            let line = Line::from(vec![
                Span::styled(format!("  {} ", snap.name), style),
                Span::styled(format!("({:.2} MB) ", size_mb), Style::default().fg(Color::Cyan)),
                Span::styled(format!("- Created: {}", snap.created_at.format("%Y-%m-%d %H:%M:%S")), Style::default().fg(Color::DarkGray)),
            ]);
            items.push(ListItem::new(line));
        }
    }

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" 📦 Snapshots Catalog — Press [L] to Refresh, [R] to Restore Selected "),
        );
    frame.render_widget(list, area);
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    let status_style = if app.status_is_error {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };

    let footer_text = vec![
        Line::from(Span::styled(&app.status_message, status_style)),
        Line::from(Span::styled(
            " [Tab] Next Tab  │  [S] Save Config  │  [T] Test Target  │  [B] Backup Now  │  [Q/Esc] Quit ",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let footer = Paragraph::new(footer_text)
        .block(Block::default().borders(Borders::ALL).title(" Status & Shortcuts "));
    frame.render_widget(footer, area);
}

fn render_live_backup_modal(frame: &mut Frame, area: Rect, app: &App) {
    let popup_area = centered_rect(60, 40, area);
    frame.render_widget(Clear, popup_area);

    let mut lines = vec![
        Line::from(Span::styled(
            "⚡ Executing Live Backup Pipeline...",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        Line::raw(""),
    ];

    for log in &app.live_backup_log {
        lines.push(Line::from(Span::styled(log, Style::default().fg(Color::White))));
    }

    let popup = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Live Job Execution ")
                .style(Style::default().bg(Color::Black)),
        )
        .wrap(Wrap { trim: true });

    frame.render_widget(popup, popup_area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
