use crate::config::{Config, DatabaseSource, FilesystemSource};
use crate::engine::retention::RetentionManager;
use crate::state::SharedVaultState;
use sb_agent_core::command_intake::{
    default_socket_path, spawn_server, CommandOutcome, CommandProgress, CommandRegistry,
    ProgressSender, SigningContext,
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
                CommandOutcome::ok(config_json(&cfg))
            }
        });
    }

    // 2. Comando: update_config
    {
        let st = state.clone();
        registry.register("update_config", move |payload, _progress_tx| {
            let st = st.clone();
            async move {
                let current_cfg =
                    match merge_config_update(st.config.read().await.clone(), &payload) {
                        Ok(cfg) => cfg,
                        Err(e) => return CommandOutcome::failed(e),
                    };

                match st.apply_new_config(current_cfg.clone()).await {
                    Ok(_) => CommandOutcome::ok(config_json(&current_cfg)),
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
                    Ok(_) => CommandOutcome::ok(config_json(&current_cfg)),
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
                    _ => {
                        return CommandOutcome::failed("Missing required field 'name'".to_string())
                    }
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
                    Ok(_) => CommandOutcome::ok(config_json(&current_cfg)),
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
                    Some(s) => {
                        format!("Initiating manual backup for source '{s}' (level: {level})...")
                    }
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
                all_snapshots.sort_by_key(|s| std::cmp::Reverse(s.created_at));
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

    // 9. Comando: update_now (Actualización bajo demanda del binario desde GitHub Releases)
    {
        registry.register("update_now", move |_payload, _progress_tx| {
            async move {
                let cfg = sb_agent_core::updater::UpdaterConfig::new(
                    "securyblack",
                    "titan-vault",
                    "titanvault",
                    env!("CARGO_PKG_VERSION"),
                );

                let result =
                    tokio::task::spawn_blocking(move || sb_agent_core::updater::check_now(&cfg)).await;

                match result {
                    Ok(Ok(true)) => {
                        std::thread::spawn(|| {
                            std::thread::sleep(std::time::Duration::from_secs(2));
                            std::process::exit(0);
                        });
                        CommandOutcome::ok(
                            serde_json::json!({ "updated": true, "previous_version": env!("CARGO_PKG_VERSION") })
                                .to_string(),
                        )
                    }
                    Ok(Ok(false)) => CommandOutcome::ok(
                        serde_json::json!({ "updated": false, "current_version": env!("CARGO_PKG_VERSION") })
                            .to_string(),
                    ),
                    Ok(Err(e)) => CommandOutcome::failed(format!("update check failed: {e}")),
                    Err(e) => CommandOutcome::failed(format!("update task panicked: {e}")),
                }
            }
        });
    }

    // 10. Comando: restore_snapshot — recupera una copia a una ruta nueva del
    // servidor (restores_dir), igual que `titanvault restore`: nunca sobrescribe
    // ni ejecuta el SQL recuperado.
    {
        let st = state.clone();
        registry.register("restore_snapshot", move |payload, _progress_tx| {
            let st = st.clone();
            async move {
                let (Some(target), Some(path)) = (
                    payload.get("target").and_then(|v| v.as_str()),
                    payload.get("path").and_then(|v| v.as_str()),
                ) else {
                    return CommandOutcome::failed("target and path are required");
                };
                let dir = restores_dir();
                if let Err(e) = std::fs::create_dir_all(&dir) {
                    return CommandOutcome::failed(format!("create {}: {e}", dir.display()));
                }
                let destination = dir.join(restore_name(path, chrono::Utc::now()));
                let cfg = st.config.read().await.clone();
                let p = st.pipeline.read().await.clone();
                match crate::engine::restore::restore_snapshot(
                    &cfg,
                    &p.storage(),
                    target,
                    path,
                    &destination,
                )
                .await
                {
                    Ok(report) => {
                        CommandOutcome::ok(serde_json::to_string(&report).unwrap_or_default())
                    }
                    Err(e) => CommandOutcome::failed(format!("Restore failed: {e:#}")),
                }
            }
        });
    }

    registry
}

fn restores_dir() -> std::path::PathBuf {
    if cfg!(windows) {
        let pd = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
        std::path::PathBuf::from(pd)
            .join("SecuryBlack")
            .join("titanvault")
            .join("restores")
    } else {
        std::path::PathBuf::from("/var/lib/titanvault/restores")
    }
}

/// `db/app-20261022.sql.zst.enc` → `20261023-010509-app-20261022.sql`; un `.tar.zst`
/// da una carpeta. La hora delante hace que dos restauraciones no choquen.
fn restore_name(path: &str, now: chrono::DateTime<chrono::Utc>) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    let plain = name.strip_suffix(".enc").unwrap_or(name);
    let base = plain
        .strip_suffix(".sql.zst")
        .map(|b| format!("{b}.sql"))
        .or_else(|| plain.strip_suffix(".tar.zst").map(String::from))
        .unwrap_or_else(|| plain.to_string());
    format!("{}-{base}", now.format("%Y%m%d-%H%M%S"))
}

/// Config as returned by the config commands, plus the host's IANA timezone:
/// schedules run in host local time, and SecuryBlack needs it to evaluate the
/// heartbeat crons that mirror them.
fn config_json(cfg: &Config) -> String {
    let mut value = serde_json::to_value(cfg).unwrap_or_default();
    if let Some(obj) = value.as_object_mut() {
        let tz = iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".to_string());
        obj.insert("host_timezone".to_string(), serde_json::Value::String(tz));
    }
    serde_json::to_string_pretty(&value).unwrap_or_default()
}

/// Applies the sections present in an `update_config` payload. Sections that
/// are absent keep their current value; an invalid section rejects the update.
/// `cloud` only accepts `heartbeat_urls` (level → URL), so the SecuryBlack
/// token and endpoint can't be changed remotely.
fn merge_config_update(mut cfg: Config, payload: &serde_json::Value) -> Result<Config, String> {
    fn section<T: serde::de::DeserializeOwned>(
        payload: &serde_json::Value,
        key: &str,
    ) -> Result<Option<T>, String> {
        payload
            .get(key)
            .map(|v| serde_json::from_value(v.clone()).map_err(|e| format!("Invalid '{key}': {e}")))
            .transpose()
    }
    if let Some(s) = section(payload, "sources")? {
        cfg.sources = s;
    }
    if let Some(t) = section(payload, "targets")? {
        cfg.targets = t;
    }
    if let Some(s) = section(payload, "schedule")? {
        cfg.schedule = s;
    }
    if let Some(r) = section(payload, "retention")? {
        cfg.retention = r;
    }
    if let Some(c) = section(payload, "crypto")? {
        cfg.crypto = c;
    }
    if let Some(cloud) = payload.get("cloud") {
        if let Some(urls) = section(cloud, "heartbeat_urls")? {
            cfg.cloud.heartbeat_urls = urls;
        }
    }
    Ok(cfg)
}

/// `server_token`: `cloud.token` del config.toml, con el que la API ata las
/// órdenes firmadas (S4) a este servidor.
pub fn start_command_intake_server(
    agent_name: &str,
    registry: CommandRegistry,
    server_token: String,
) {
    let sock_path = default_socket_path(agent_name);
    spawn_server(
        registry,
        sock_path,
        SigningContext {
            server_token,
            target_agent: "titanvault".into(),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base() -> Config {
        toml::from_str(
            "[cloud]\ntoken = 'secret'\n[[sources.filesystems]]\nname = 'etc'\npaths = ['/etc']\n[targets.local]\npath = '/var/backups'",
        )
        .unwrap()
    }

    #[test]
    fn partial_update_keeps_other_sections() {
        let cfg = merge_config_update(base(), &json!({"retention": {"keep_hourly": 5}})).unwrap();
        assert_eq!(cfg.retention.keep_hourly, 5);
        assert_eq!(cfg.sources.filesystems.len(), 1);
        assert!(cfg.targets.local.is_some());
    }

    #[test]
    fn cloud_update_only_touches_heartbeat_urls() {
        let payload =
            json!({"cloud": {"token": "evil", "heartbeat_urls": {"daily": "https://x.test/d"}}});
        let cfg = merge_config_update(base(), &payload).unwrap();
        assert_eq!(
            cfg.cloud.heartbeat_for_level("daily"),
            Some("https://x.test/d")
        );
        assert_eq!(cfg.cloud.token.as_deref(), Some("secret"));
        assert_eq!(cfg.sources.filesystems.len(), 1);
    }

    #[test]
    fn invalid_section_is_rejected() {
        assert!(
            merge_config_update(base(), &json!({"cloud": {"heartbeat_urls": ["no"]}})).is_err()
        );
    }

    #[test]
    fn restore_names_are_new_paths() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-23T01:05:09Z")
            .unwrap()
            .into();
        assert_eq!(
            restore_name("db/app-1.sql.zst.enc", now),
            "20261023-010509-app-1.sql"
        );
        assert_eq!(
            restore_name("fs/www-1.tar.zst", now),
            "20261023-010509-www-1"
        );
        assert_eq!(restore_name("weird.bin", now), "20261023-010509-weird.bin");
    }

    #[test]
    fn config_json_reports_host_timezone() {
        let v: serde_json::Value = serde_json::from_str(&config_json(&base())).unwrap();
        assert!(v["host_timezone"].as_str().is_some_and(|tz| !tz.is_empty()));
    }
}
