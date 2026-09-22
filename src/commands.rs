use crate::config::{Config, DatabaseSource, FilesystemSource};
use crate::engine::retention::RetentionManager;
use crate::state::SharedVaultState;
use sb_agent_core::command_intake::{
    default_socket_path, spawn_server, CommandOutcome, CommandProgress, CommandRegistry,
    ProgressSender,
};
use std::sync::Arc;

pub fn build_command_registry(state: Arc<SharedVaultState>) -> CommandRegistry {
    let registry = CommandRegistry::new();

    // 1. Comando: get_config
    {
        let st = state.clone();
        registry.register("get_config", move |_payload, _progress_tx| {
            let st = st.clone();
            async move {
                let cfg = st.config.read().await;
                match serde_json::to_string_pretty(&*cfg) {
                    Ok(json) => CommandOutcome::ok(json),
                    Err(e) => CommandOutcome::failed(format!("Failed to serialize config: {e}")),
                }
            }
        });
    }

    // 2. Comando: update_config
    {
        let st = state.clone();
        registry.register("update_config", move |payload, _progress_tx| {
            let st = st.clone();
            async move {
                let mut current_cfg = st.config.read().await.clone();

                // Permitir actualización completa o por secciones
                if let Some(sources) = payload.get("sources") {
                    if let Ok(s) = serde_json::from_value(sources.clone()) {
                        current_cfg.sources = s;
                    }
                }
                if let Some(targets) = payload.get("targets") {
                    if let Ok(t) = serde_json::from_value(targets.clone()) {
                        current_cfg.targets = t;
                    }
                }
                if let Some(sched) = payload.get("schedule") {
                    if let Ok(s) = serde_json::from_value(sched.clone()) {
                        current_cfg.schedule = s;
                    }
                }
                if let Some(ret) = payload.get("retention") {
                    if let Ok(r) = serde_json::from_value(ret.clone()) {
                        current_cfg.retention = r;
                    }
                }
                if let Some(crypto) = payload.get("crypto") {
                    if let Ok(c) = serde_json::from_value(crypto.clone()) {
                        current_cfg.crypto = c;
                    }
                }

                // Si se proporcionó el objeto Config completo
                if payload.get("sources").is_none()
                    && payload.get("targets").is_none()
                    && payload.get("schedule").is_none()
                {
                    if let Ok(c) = serde_json::from_value::<Config>(payload.clone()) {
                        current_cfg = c;
                    }
                }

                match st.apply_new_config(current_cfg.clone()).await {
                    Ok(_) => {
                        let json = serde_json::to_string_pretty(&current_cfg).unwrap_or_default();
                        CommandOutcome::ok(json)
                    }
                    Err(e) => CommandOutcome::failed(format!("Failed to apply configuration: {e}")),
                }
            }
        });
    }

    // 3. Comando: add_source
    {
        let st = state.clone();
        registry.register("add_source", move |payload, _progress_tx| {
            let st = st.clone();
            async move {
                let source_type = payload
                    .get("source_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("filesystem");

                let mut current_cfg = st.config.read().await.clone();

                match source_type {
                    "filesystem" => {
                        let fs_val = payload
                            .get("filesystem")
                            .or_else(|| payload.get("source"))
                            .unwrap_or(&payload);

                        match serde_json::from_value::<FilesystemSource>(fs_val.clone()) {
                            Ok(fs) => {
                                // Reemplazar si ya existe con el mismo nombre o añadir
                                if let Some(pos) = current_cfg
                                    .sources
                                    .filesystems
                                    .iter()
                                    .position(|x| x.name == fs.name)
                                {
                                    current_cfg.sources.filesystems[pos] = fs;
                                } else {
                                    current_cfg.sources.filesystems.push(fs);
                                }
                            }
                            Err(e) => {
                                return CommandOutcome::failed(format!(
                                    "Invalid filesystem source payload: {e}"
                                ))
                            }
                        }
                    }
                    "database" => {
                        let db_val = payload
                            .get("database")
                            .or_else(|| payload.get("source"))
                            .unwrap_or(&payload);

                        match serde_json::from_value::<DatabaseSource>(db_val.clone()) {
                            Ok(db) => {
                                if let Some(pos) = current_cfg
                                    .sources
                                    .databases
                                    .iter()
                                    .position(|x| x.name == db.name)
                                {
                                    current_cfg.sources.databases[pos] = db;
                                } else {
                                    current_cfg.sources.databases.push(db);
                                }
                            }
                            Err(e) => {
                                return CommandOutcome::failed(format!(
                                    "Invalid database source payload: {e}"
                                ))
                            }
                        }
                    }
                    other => {
                        return CommandOutcome::failed(format!("Unsupported source_type: {other}"))
                    }
                }

                match st.apply_new_config(current_cfg.clone()).await {
                    Ok(_) => {
                        let json = serde_json::to_string_pretty(&current_cfg).unwrap_or_default();
                        CommandOutcome::ok(json)
                    }
                    Err(e) => CommandOutcome::failed(format!("Failed to save new source: {e}")),
                }
            }
        });
    }

    // 4. Comando: delete_source
    {
        let st = state.clone();
        registry.register("delete_source", move |payload, _progress_tx| {
            let st = st.clone();
            async move {
                let name = match payload.get("name").and_then(|v| v.as_str()) {
                    Some(n) if !n.is_empty() => n,
                    _ => return CommandOutcome::failed("Missing required field 'name'".to_string()),
                };

                let source_type = payload
                    .get("source_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("filesystem");

                let mut current_cfg = st.config.read().await.clone();

                match source_type {
                    "filesystem" => {
                        let before = current_cfg.sources.filesystems.len();
                        current_cfg.sources.filesystems.retain(|x| x.name != name);
                        if current_cfg.sources.filesystems.len() == before {
                            return CommandOutcome::failed(format!(
                                "Filesystem source '{}' not found",
                                name
                            ));
                        }
                    }
                    "database" => {
                        let before = current_cfg.sources.databases.len();
                        current_cfg.sources.databases.retain(|x| x.name != name);
                        if current_cfg.sources.databases.len() == before {
                            return CommandOutcome::failed(format!(
                                "Database source '{}' not found",
                                name
                            ));
                        }
                    }
                    _ => {
                        // Buscar en ambos
                        current_cfg.sources.filesystems.retain(|x| x.name != name);
                        current_cfg.sources.databases.retain(|x| x.name != name);
                    }
                }

                match st.apply_new_config(current_cfg.clone()).await {
                    Ok(_) => {
                        let json = serde_json::to_string_pretty(&current_cfg).unwrap_or_default();
                        CommandOutcome::ok(json)
                    }
                    Err(e) => CommandOutcome::failed(format!("Failed to delete source: {e}")),
                }
            }
        });
    }

    // 5. Comando: backup_now (global o por origen específico)
    {
        let st = state.clone();
        registry.register("backup_now", move |payload, progress_tx: ProgressSender| {
            let st = st.clone();
            async move {
                let level = payload
                    .get("level")
                    .and_then(|v| v.as_str())
                    .unwrap_or("daily");

                let source_name = payload
                    .get("source_name")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty());

                let desc = match source_name {
                    Some(s) => format!("Initiating manual backup for source '{s}' (level: {level})..."),
                    None => format!("Initiating full manual backup (level: {level})..."),
                };

                let _ = progress_tx.send(CommandProgress {
                    command_id: "".to_string(),
                    stage: "starting".to_string(),
                    message: desc,
                    percent: 10,
                });

                let p = st.pipeline.read().await.clone();

                let reports = match source_name {
                    Some(name) => p.run_source(name, level).await,
                    None => p.run_all(level).await,
                };

                let _ = progress_tx.send(CommandProgress {
                    command_id: "".to_string(),
                    stage: "finished".to_string(),
                    message: "Backup job completed".to_string(),
                    percent: 100,
                });

                let all_ok = !reports.is_empty() && reports.iter().all(|r| r.success);
                let json_output = serde_json::to_string_pretty(&reports).unwrap_or_default();

                if all_ok {
                    CommandOutcome::ok(json_output)
                } else if reports.is_empty() {
                    CommandOutcome::failed("No backup sources matched or were active.".to_string())
                } else {
                    CommandOutcome::failed(format!(
                        "One or more backup jobs failed:\n{}",
                        json_output
                    ))
                }
            }
        });
    }

    // 6. Comando: list_snapshots
    {
        let st = state.clone();
        registry.register("list_snapshots", move |payload, _progress_tx| {
            let st = st.clone();
            async move {
                let target_param = payload.get("target").and_then(|v| v.as_str());

                let p = st.pipeline.read().await.clone();
                let storage = p.storage();
                let available_targets = storage.get_targets();

                if available_targets.is_empty() {
                    return CommandOutcome::ok("[]".to_string());
                }

                let mut all_snapshots = Vec::new();

                if let Some(target_id) = target_param {
                    if target_id != "all" && storage.get_target(target_id).is_some() {
                        match storage.list_snapshots(target_id).await {
                            Ok(snapshots) => {
                                let json =
                                    serde_json::to_string_pretty(&snapshots).unwrap_or_default();
                                return CommandOutcome::ok(json);
                            }
                            Err(e) => {
                                return CommandOutcome::failed(format!(
                                    "Failed to list snapshots on {}: {:#}",
                                    target_id, e
                                ))
                            }
                        }
                    }
                }

                // If target not specified or "all", query all active targets
                for target in available_targets {
                    if let Ok(snapshots) = storage.list_snapshots(&target.id).await {
                        all_snapshots.extend(snapshots);
                    }
                }
                all_snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                let json = serde_json::to_string_pretty(&all_snapshots).unwrap_or_default();
                CommandOutcome::ok(json)
            }
        });
    }

    // 7. Comando: prune
    {
        let st = state.clone();
        registry.register("prune", move |payload, _progress_tx| {
            let st = st.clone();
            async move {
                let target_id = payload
                    .get("target")
                    .and_then(|v| v.as_str())
                    .unwrap_or("hetzner");

                let p = st.pipeline.read().await.clone();
                let ret = st.config.read().await.retention.clone();

                match RetentionManager::prune_target(&p.storage(), target_id, &ret).await {
                    Ok(deleted) => CommandOutcome::ok(format!(
                        r#"{{"target": "{}", "deleted_count": {}}}"#,
                        target_id, deleted
                    )),
                    Err(e) => {
                        CommandOutcome::failed(format!("Pruning failed on {}: {:#}", target_id, e))
                    }
                }
            }
        });
    }

    // 8. Comando: test_target
    {
        let st = state.clone();
        registry.register("test_target", move |payload, _progress_tx| {
            let st = st.clone();
            async move {
                let target_id = payload
                    .get("target")
                    .and_then(|v| v.as_str())
                    .unwrap_or("hetzner");

                let p = st.pipeline.read().await.clone();

                match p.storage().test_connection(target_id).await {
                    Ok(_) => CommandOutcome::ok(format!(
                        r#"{{"target": "{}", "status": "connected"}}"#,
                        target_id
                    )),
                    Err(e) => CommandOutcome::failed(format!(
                        "Connection test failed on {}: {:#}",
                        target_id, e
                    )),
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
