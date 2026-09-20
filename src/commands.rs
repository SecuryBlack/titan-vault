use crate::config::Config;
use crate::engine::pipeline::BackupPipeline;
use crate::engine::retention::RetentionManager;
use sb_agent_core::command_intake::{
    default_socket_path, spawn_server, CommandOutcome, CommandProgress, CommandRegistry,
    ProgressSender,
};
use std::sync::Arc;

pub fn build_command_registry(
    config: Config,
    pipeline: Arc<BackupPipeline>,
) -> CommandRegistry {
    let registry = CommandRegistry::new();

    // 1. Comando: backup_now
    {
        let p = pipeline.clone();
        registry.register("backup_now", move |payload, progress_tx: ProgressSender| {
            let p = p.clone();
            async move {
                let level = payload
                    .get("level")
                    .and_then(|v| v.as_str())
                    .unwrap_or("daily");

                let _ = progress_tx.send(CommandProgress {
                    command_id: "".to_string(),
                    stage: "starting".to_string(),
                    message: format!("Initiating manual backup (level: {})...", level),
                    percent: 10,
                });

                let reports = p.run_all(level).await;

                let _ = progress_tx.send(CommandProgress {
                    command_id: "".to_string(),
                    stage: "finished".to_string(),
                    message: "All backup jobs completed".to_string(),
                    percent: 100,
                });

                let all_ok = reports.iter().all(|r| r.success);
                let json_output = serde_json::to_string_pretty(&reports).unwrap_or_default();

                if all_ok {
                    CommandOutcome::ok(json_output)
                } else {
                    CommandOutcome::failed(format!("One or more backup jobs failed:\n{}", json_output))
                }
            }
        });
    }

    // 2. Comando: list_snapshots
    {
        let p = pipeline.clone();
        registry.register("list_snapshots", move |payload, _progress_tx| {
            let p = p.clone();
            async move {
                let target_id = payload
                    .get("target")
                    .and_then(|v| v.as_str())
                    .unwrap_or("hetzner");

                match p.storage().list_snapshots(target_id).await {
                    Ok(snapshots) => {
                        let json = serde_json::to_string_pretty(&snapshots).unwrap_or_default();
                        CommandOutcome::ok(json)
                    }
                    Err(e) => CommandOutcome::failed(format!("Failed to list snapshots: {:#}", e)),
                }
            }
        });
    }

    // 3. Comando: prune
    {
        let p = pipeline.clone();
        let retention = config.retention.clone();
        registry.register("prune", move |payload, _progress_tx| {
            let p = p.clone();
            let ret = retention.clone();
            async move {
                let target_id = payload
                    .get("target")
                    .and_then(|v| v.as_str())
                    .unwrap_or("hetzner");

                match RetentionManager::prune_target(&p.storage(), target_id, &ret).await {
                    Ok(deleted) => CommandOutcome::ok(format!(
                        r#"{{"target": "{}", "deleted_count": {}}}"#,
                        target_id, deleted
                    )),
                    Err(e) => CommandOutcome::failed(format!("Pruning failed on {}: {:#}", target_id, e)),
                }
            }
        });
    }

    // 4. Comando: test_target
    {
        let p = pipeline.clone();
        registry.register("test_target", move |payload, _progress_tx| {
            let p = p.clone();
            async move {
                let target_id = payload
                    .get("target")
                    .and_then(|v| v.as_str())
                    .unwrap_or("hetzner");

                match p.storage().test_connection(target_id).await {
                    Ok(_) => CommandOutcome::ok(format!(r#"{{"target": "{}", "status": "connected"}}"#, target_id)),
                    Err(e) => CommandOutcome::failed(format!("Connection test failed on {}: {:#}", target_id, e)),
                }
            }
        });
    }

    registry
}

pub fn start_command_intake_server(agent_name: &str, registry: CommandRegistry) {
    let sock_path = default_socket_path(agent_name);
    spawn_server(registry, sock_path);
}
