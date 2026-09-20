pub mod app;
pub mod ui;

use app::{App, Tab};
use crate::config::Config;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{execute, ExecutableCommand};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::stdout;
use std::path::PathBuf;
use std::time::Duration;

pub async fn run_tui(config_path: Option<PathBuf>) -> std::io::Result<()> {
    let resolved_path = config_path.unwrap_or_else(|| sb_agent_core::config::default_config_path(crate::config::AGENT_NAME));
    let config = Config::load_from(&resolved_path).unwrap_or_default();

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(config, resolved_path);
    let res = event_loop(&mut terminal, &mut app).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    res
}

async fn event_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> std::io::Result<()> {
    loop {
        terminal.draw(|frame| ui::render(frame, app))?;

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            if app.live_backup_in_progress {
                                app.live_backup_in_progress = false;
                            } else {
                                return Ok(());
                            }
                        }
                        KeyCode::Tab => {
                            app.next_tab();
                        }
                        KeyCode::BackTab => {
                            app.prev_tab();
                        }
                        KeyCode::Char('1') => app.current_tab = Tab::Sources,
                        KeyCode::Char('2') => app.current_tab = Tab::Targets,
                        KeyCode::Char('3') => app.current_tab = Tab::PolicyCrypto,
                        KeyCode::Char('4') => app.current_tab = Tab::Snapshots,
                        KeyCode::Up => app.move_up(),
                        KeyCode::Down => app.move_down(10),
                        KeyCode::Char('s') | KeyCode::Char('S') => {
                            app.save_config();
                        }
                        KeyCode::Char('t') | KeyCode::Char('T') => {
                            // Ejecutar test de conexión en caliente
                            if let Some(p) = &app.pipeline {
                                let storage = p.storage();
                                app.status_message = "Testing connection to targets...".to_string();
                                let targets = storage.get_targets();
                                if targets.is_empty() {
                                    app.test_result = Some("No targets active to test.".to_string());
                                } else {
                                    let mut results = Vec::new();
                                    for t in targets {
                                        match storage.test_connection(&t.id).await {
                                            Ok(_) => results.push(format!("{}: OK", t.id)),
                                            Err(e) => results.push(format!("{}: FAIL ({})", t.id, e)),
                                        }
                                    }
                                    app.test_result = Some(results.join(" | "));
                                }
                            } else {
                                app.test_result = Some("Pipeline not initialized.".to_string());
                            }
                        }
                        KeyCode::Char('b') | KeyCode::Char('B') => {
                            // Disparar backup en caliente
                            if let Some(p) = &app.pipeline {
                                app.live_backup_in_progress = true;
                                app.live_backup_log.clear();
                                app.live_backup_log.push("Initiating on-demand backup pipeline...".to_string());
                                terminal.draw(|frame| ui::render(frame, app))?;

                                let p = p.clone();
                                let reports = p.run_all("manual").await;
                                for r in reports {
                                    let status = if r.success { "SUCCESS" } else { "FAILED" };
                                    app.live_backup_log.push(format!(
                                        "[{}] {} ({}) -> {} in {:.2}s",
                                        status, r.source_name, r.source_type, r.file_name, r.duration_secs
                                    ));
                                }
                                app.live_backup_log.push("Job complete. Press [Esc] to return.".to_string());
                            }
                        }
                        KeyCode::Char('l') | KeyCode::Char('L') => {
                            // Refrescar catálogo de snapshots
                            if let Some(p) = &app.pipeline {
                                app.status_message = "Fetching snapshots...".to_string();
                                let targets = p.storage().get_targets();
                                let mut all_snaps = Vec::new();
                                for t in targets {
                                    if let Ok(snaps) = p.storage().list_snapshots(&t.id).await {
                                        all_snaps.extend(snaps);
                                    }
                                }
                                app.snapshots = all_snaps;
                                app.status_message = format!("Loaded {} snapshots.", app.snapshots.len());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}
