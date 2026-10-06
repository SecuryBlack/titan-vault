mod commands;
mod config;
mod engine;
mod state;
mod storage;
mod tui;

use config::{Config, AGENT_NAME, BIN_NAME, VERSION};
use engine::pipeline::BackupPipeline;
use engine::scheduler::AutonomousScheduler;
use state::SharedVaultState;
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

    let config_path = sb_agent_core::config::default_config_path(AGENT_NAME);
    let log_dir = config_path
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

    // 3. Estado Compartido Reactivo
    let shared_state = Arc::new(SharedVaultState::new(
        cfg.clone(),
        pipeline.clone(),
        config_path,
        Some(status_handle.clone()),
    ));

    // 4. Command Intake Socket (para recibir órdenes de nexus-agent / SecuryBlack Cloud)
    let command_registry = commands::build_command_registry(shared_state.clone());
    commands::start_command_intake_server(AGENT_NAME, command_registry);

    status_handle.set_state("running");
    status_handle.set_details(serde_json::json!({
        "mode": cfg.mode,
        "databases_count": cfg.sources.databases.len(),
        "filesystems_count": cfg.sources.filesystems.len(),
        "targets_count": pipeline.storage().get_targets().len(),
        "schedule_enabled": cfg.schedule.enabled,
    }));

    // 5. Comprobador de actualizaciones en segundo plano (GitHub Releases)
    sb_agent_core::updater::start_daily_check(sb_agent_core::updater::UpdaterConfig::new(
        "securyblack",
        "titan-vault",
        "titanvault",
        VERSION,
    ));

    // 6. Scheduler Autónomo
    let scheduler = AutonomousScheduler::new(shared_state);
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

fn load_cli_config(path: Option<&std::path::Path>) -> anyhow::Result<Config> {
    match path {
        Some(p) => Config::load_from(p),
        None => Config::load_default(),
    }
    .map_err(|e| anyhow::anyhow!("failed to load backup configuration: {e}"))
}

async fn run_cli_backup(level: &str, config_path: Option<&std::path::Path>) -> anyhow::Result<()> {
    if !["hourly", "daily", "weekly", "monthly", "yearly"].contains(&level) {
        return Err(anyhow::anyhow!("invalid backup level: {level}"));
    }
    let p = BackupPipeline::new(load_cli_config(config_path)?)?;
    let reports = p.run_all(level).await;
    println!("{}", serde_json::to_string_pretty(&reports)?);
    if reports.is_empty() || reports.iter().any(|r| !r.success) {
        return Err(anyhow::anyhow!(
            "backup failed or no active sources were configured"
        ));
    }
    Ok(())
}

async fn run_cli_prune(
    level: Option<&str>,
    config_path: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    let cfg = load_cli_config(config_path)?;
    let p = BackupPipeline::new(cfg.clone())?;
    let targets = p.storage().get_targets();
    if targets.is_empty() {
        return Err(anyhow::anyhow!("no enabled storage targets"));
    }
    for target in targets {
        let count = if let Some(level) = level {
            if !["hourly", "daily", "weekly", "monthly", "yearly"].contains(&level) {
                return Err(anyhow::anyhow!("invalid retention level"));
            }
            engine::retention::RetentionManager::prune_target_level(
                &p.storage(),
                &target.id,
                &cfg.retention,
                level,
            )
            .await?
        } else {
            engine::retention::RetentionManager::prune_target(
                &p.storage(),
                &target.id,
                &cfg.retention,
            )
            .await?
        };
        println!("{}: {count} snapshots pruned", target.id);
    }
    Ok(())
}

async fn run_cli_test(config_path: Option<&std::path::Path>) -> anyhow::Result<()> {
    let p = BackupPipeline::new(load_cli_config(config_path)?)?;
    let targets = p.storage().get_targets();
    if targets.is_empty() {
        return Err(anyhow::anyhow!("no enabled storage targets"));
    }
    for target in targets {
        p.storage().test_connection(&target.id).await?;
        println!("{}: connected", target.id);
    }
    Ok(())
}

fn required_option<'a>(args: &'a [String], option: &str) -> anyhow::Result<&'a str> {
    args.iter()
        .position(|arg| arg == option)
        .and_then(|i| args.get(i + 1))
        .filter(|value| !value.starts_with("--"))
        .map(String::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing {option} value"))
}

async fn run_cli_restore(
    args: &[String],
    config_path: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    let cfg = load_cli_config(config_path)?;
    let storage = storage::StorageManager::new(&cfg)?;
    let report = engine::restore::restore_snapshot(
        &cfg,
        &storage,
        required_option(args, "--target")?,
        required_option(args, "--snapshot")?,
        std::path::Path::new(required_option(args, "--destination")?),
    )
    .await?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

async fn run_cli_snapshots(
    args: &[String],
    config_path: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    let storage = storage::StorageManager::new(&load_cli_config(config_path)?)?;
    let snapshots = storage
        .list_snapshots(required_option(args, "--target")?)
        .await?;
    println!("{}", serde_json::to_string_pretty(&snapshots)?);
    Ok(())
}

fn cli_exit(result: anyhow::Result<()>) -> ! {
    match result {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            eprintln!("[{BIN_NAME}] {e:#}");
            std::process::exit(1);
        }
    }
}

fn handle_cli_args() {
    // 1. Primero comprobar --version, status y top comunes de sb-agent-core
    sb_agent_core::cli::dispatch_common_args(AGENT_NAME, BIN_NAME, VERSION);

    let args: Vec<String> = std::env::args().collect();
    let config_path = if args.iter().any(|a| a == "--config") {
        Some(std::path::PathBuf::from(
            required_option(&args, "--config").unwrap_or_else(|e| {
                eprintln!("{e}");
                std::process::exit(1)
            }),
        ))
    } else {
        None
    };

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
                let level = args
                    .get(2)
                    .filter(|value| !value.starts_with("--"))
                    .map(|s| s.as_str())
                    .unwrap_or("daily");
                let rt = tokio::runtime::Runtime::new().expect("failed to start tokio runtime");
                cli_exit(rt.block_on(run_cli_backup(level, config_path.as_deref())));
            }
            "restore" => {
                let rt = tokio::runtime::Runtime::new().expect("failed to start tokio runtime");
                cli_exit(rt.block_on(run_cli_restore(&args, config_path.as_deref())));
            }
            "snapshots" => {
                let rt = tokio::runtime::Runtime::new().expect("failed to start tokio runtime");
                cli_exit(rt.block_on(run_cli_snapshots(&args, config_path.as_deref())));
            }
            "prune" => {
                let rt = tokio::runtime::Runtime::new().expect("failed to start tokio runtime");
                let level = if args.iter().any(|arg| arg == "--level") {
                    Some(required_option(&args, "--level").unwrap_or_else(|e| {
                        eprintln!("{e}");
                        std::process::exit(1)
                    }))
                } else {
                    None
                };
                cli_exit(rt.block_on(run_cli_prune(level, config_path.as_deref())));
            }
            "test" => {
                let rt = tokio::runtime::Runtime::new().expect("failed to start tokio runtime");
                cli_exit(rt.block_on(run_cli_test(config_path.as_deref())));
            }
            "help" | "--help" | "-h" => {
                println!(
                    "TitanVault v{}\n\
                    SecuryBlack storage, backup and disaster recovery agent\n\n\
                    USAGE:\n\
                      titanvault [COMMAND]\n\n\
                    COMMANDS:\n\
                      tui, config       Open interactive standalone Terminal User Interface (Ratatui)\n\
                      backup [LEVEL]    Run on-demand backup (hourly, daily, weekly, monthly, yearly)\n\
                      snapshots --target TARGET   List relative snapshot paths\n\
                      restore --target TARGET --snapshot PATH --destination NEW_PATH\n\
                                        Recover files or export SQL, never overwrite a destination\n\
                      prune             Apply retention to configured targets\n\
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
