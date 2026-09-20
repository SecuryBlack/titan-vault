mod commands;
mod config;
mod engine;
mod storage;
mod tui;

use config::{Config, AGENT_NAME, BIN_NAME, VERSION};
use engine::pipeline::BackupPipeline;
use engine::scheduler::AutonomousScheduler;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::info;

async fn run(shutdown: tokio::sync::oneshot::Receiver<()>) {
    let cfg = match Config::load_default() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[{BIN_NAME}] Failed to load configuration: {e}");
            std::process::exit(1);
        }
    };

    let log_dir = sb_agent_core::config::default_config_path(AGENT_NAME)
        .parent()
        .expect("config path always has a parent")
        .to_path_buf();
    sb_agent_core::logging::init(AGENT_NAME, &log_dir, "info");

    info!(
        mode = %cfg.mode,
        version = %cfg.version,
        "TitanVault daemon starting..."
    );

    // 1. Status Socket (para que nexus-agent y `titanvault status`/`top` lo lean)
    let status_handle = sb_agent_core::status::StatusHandle::new(AGENT_NAME, VERSION);
    let status_sock_path = sb_agent_core::status::default_socket_path(AGENT_NAME);
    sb_agent_core::status::spawn_server(status_handle.clone(), status_sock_path);
    status_handle.set_state("starting");

    // 2. Inicializar Pipeline de Backup
    let pipeline = match BackupPipeline::new(cfg.clone()) {
        Ok(p) => Arc::new(p),
        Err(e) => {
            eprintln!("[{BIN_NAME}] Failed to initialize backup pipeline: {e:#}");
            std::process::exit(1);
        }
    };

    // 3. Command Intake Socket (para recibir órdenes de nexus-agent / SecuryBlack Cloud)
    let command_registry = commands::build_command_registry(cfg.clone(), pipeline.clone());
    commands::start_command_intake_server(AGENT_NAME, command_registry);

    status_handle.set_state("running");
    status_handle.set_details(serde_json::json!({
        "mode": cfg.mode,
        "databases_count": cfg.sources.databases.len(),
        "filesystems_count": cfg.sources.filesystems.len(),
        "targets_count": pipeline.storage().get_targets().len(),
        "schedule_enabled": cfg.schedule.enabled,
    }));

    // 4. Scheduler Autónomo
    let scheduler = AutonomousScheduler::new(cfg, pipeline);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let sched_handle = tokio::spawn(async move {
        let _ = scheduler.run(shutdown_rx).await;
    });

    // Esperar señal de parada
    let _ = shutdown.await;
    info!("Shutdown signal received. Stopping TitanVault daemon...");
    status_handle.set_state("stopping");
    let _ = shutdown_tx.send(true);
    let _ = sched_handle.await;
    info!("TitanVault stopped cleanly.");
}

async fn run_cli_backup(level: &str, config_path: Option<&std::path::Path>) {
    let cfg = if let Some(p) = config_path {
        Config::load_from(p).unwrap_or_default()
    } else {
        Config::load_default().unwrap_or_default()
    };
    println!("📦 TitanVault — Running on-demand backup (level: {})...", level);
    match BackupPipeline::new(cfg) {
        Ok(p) => {
            let reports = p.run_all(level).await;
            for r in reports {
                let status = if r.success { "✅ SUCCESS" } else { "❌ FAILED" };
                println!(
                    "{} {} ({}) -> {} [{:.2} MB in {:.2}s, -{:.1}%]",
                    status,
                    r.source_name,
                    r.source_type,
                    r.file_name,
                    r.final_bytes as f64 / (1024.0 * 1024.0),
                    r.duration_secs,
                    r.compression_ratio
                );
            }
        }
        Err(e) => eprintln!("Error initializing backup pipeline: {e:#}"),
    }
}

async fn run_cli_test(config_path: Option<&std::path::Path>) {
    let cfg = if let Some(p) = config_path {
        Config::load_from(p).unwrap_or_default()
    } else {
        Config::load_default().unwrap_or_default()
    };
    println!("🔍 TitanVault — Testing connectivity to configured storage targets...");
    match BackupPipeline::new(cfg) {
        Ok(p) => {
            let targets = p.storage().get_targets();
            if targets.is_empty() {
                println!("⚠️ No storage targets are currently enabled in config.");
                return;
            }
            for t in targets {
                print!("  • Testing target '{}' ({}) ... ", t.id, t.name);
                match p.storage().test_connection(&t.id).await {
                    Ok(_) => println!("✅ OK (Connected)"),
                    Err(e) => println!("❌ FAILED: {:#}", e),
                }
            }
        }
        Err(e) => eprintln!("Error: {e:#}"),
    }
}

fn handle_cli_args() {
    // 1. Primero comprobar --version, status y top comunes de sb-agent-core
    sb_agent_core::cli::dispatch_common_args(AGENT_NAME, BIN_NAME, VERSION);

    let args: Vec<String> = std::env::args().collect();
    let config_path = args
        .iter()
        .position(|a| a == "--config")
        .and_then(|idx| args.get(idx + 1))
        .map(std::path::PathBuf::from);

    if args.len() > 1 {
        match args[1].as_str() {
            "tui" | "config" => {
                let rt = tokio::runtime::Runtime::new().expect("failed to start tokio runtime");
                if let Err(e) = rt.block_on(tui::run_tui(config_path)) {
                    eprintln!("[{BIN_NAME}] TUI error: {e}");
                }
                std::process::exit(0);
            }
            "backup" => {
                let level = args.get(2).map(|s| s.as_str()).unwrap_or("daily");
                let rt = tokio::runtime::Runtime::new().expect("failed to start tokio runtime");
                rt.block_on(run_cli_backup(level, config_path.as_deref()));
                std::process::exit(0);
            }
            "test" => {
                let rt = tokio::runtime::Runtime::new().expect("failed to start tokio runtime");
                rt.block_on(run_cli_test(config_path.as_deref()));
                std::process::exit(0);
            }
            "help" | "--help" | "-h" => {
                println!(
                    "TitanVault v{}\n\
                    SecuryBlack storage, backup and disaster recovery agent\n\n\
                    USAGE:\n\
                      titanvault [COMMAND]\n\n\
                    COMMANDS:\n\
                      tui, config       Open interactive standalone Terminal User Interface (Ratatui)\n\
                      backup [LEVEL]    Run on-demand backup (hourly, daily, weekly, monthly)\n\
                      test              Test connectivity to configured storage targets\n\
                      status            Query status socket and print JSON (from sb-agent-core)\n\
                      top               Monitor agent status live in terminal (from sb-agent-core)\n\
                      service run       Run daemon service (default if no command provided)\n\
                      --version, -V     Print version information\n",
                    VERSION
                );
                std::process::exit(0);
            }
            _ => {}
        }
    }
}

#[cfg(windows)]
fn main() {
    handle_cli_args();
    match sb_agent_core::service::windows::run_service("TitanVault", run) {
        Ok(_) => {}
        Err(e) if sb_agent_core::service::windows::is_not_started_by_scm(&e) => {
            sb_agent_core::service::run_console(run);
        }
        Err(e) => {
            eprintln!("[{BIN_NAME}] Service error: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(windows))]
fn main() {
    handle_cli_args();
    sb_agent_core::service::run_console(run);
}
