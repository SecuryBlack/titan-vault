use crate::config::RetentionConfig;
use crate::storage::{SnapshotMeta, StorageManager};
use anyhow::Result;
use std::collections::HashMap;
use tracing::info;

pub struct RetentionManager;

impl RetentionManager {
    /// Aplica la rotación GFS (Grandfather-Father-Son) en todos los destinos de almacenamiento
    pub async fn prune_target(
        storage: &StorageManager,
        target_id: &str,
        retention: &RetentionConfig,
    ) -> Result<usize> {
        Self::prune_selected(storage, target_id, retention, None).await
    }

    pub async fn prune_target_level(
        storage: &StorageManager,
        target_id: &str,
        retention: &RetentionConfig,
        level: &str,
    ) -> Result<usize> {
        Self::prune_selected(storage, target_id, retention, Some(level)).await
    }

    async fn prune_selected(
        storage: &StorageManager,
        target_id: &str,
        retention: &RetentionConfig,
        selected_level: Option<&str>,
    ) -> Result<usize> {
        info!("Running GFS retention pruning on target '{}'...", target_id);
        let snapshots = storage.list_snapshots(target_id).await?;

        // Agrupar por nivel (hourly, daily, weekly, monthly, yearly)
        let mut by_level: HashMap<String, Vec<SnapshotMeta>> = HashMap::new();

        for snap in snapshots {
            if !(snap.name.ends_with(".sql.zst")
                || snap.name.ends_with(".sql.zst.enc")
                || snap.name.ends_with(".tar.zst")
                || snap.name.ends_with(".tar.zst.enc"))
            {
                continue;
            }
            let parent = snap.path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
            let level = Self::detect_level(&snap.name);
            if selected_level.is_some_and(|selected| selected != level) {
                continue;
            }
            let marker = format!("_{level}_");
            let Some((source, _)) = snap.name.rsplit_once(&marker) else {
                continue;
            };
            let group = format!("{parent}|{source}|{level}");
            by_level.entry(group).or_default().push(snap);
        }

        let mut total_deleted = 0;

        for (group, mut snaps) in by_level {
            let level = group.rsplit('|').next().unwrap_or("");
            // Ordenar por fecha descendente (los más recientes primero)
            snaps.sort_by(|a, b| b.created_at.cmp(&a.created_at));

            let keep_limit = match level {
                "hourly" => retention.keep_hourly,
                "daily" => retention.keep_daily,
                "weekly" => retention.keep_weekly,
                "monthly" => retention.keep_monthly,
                "yearly" => retention.keep_yearly,
                _ => retention.keep_daily,
            };

            if snaps.len() > keep_limit {
                let to_delete = &snaps[keep_limit..];
                for snap in to_delete {
                    info!(
                        "Pruning expired snapshot '{}' (created at {}) from target '{}'",
                        snap.name, snap.created_at, target_id
                    );
                    storage.delete(target_id, &snap.path).await?;
                    total_deleted += 1;
                }
            }
        }

        info!(
            "Retention pruning complete on '{}': {} expired snapshots deleted",
            target_id, total_deleted
        );
        Ok(total_deleted)
    }

    fn detect_level(name: &str) -> String {
        if name.contains("_hourly_") {
            "hourly".to_string()
        } else if name.contains("_daily_") {
            "daily".to_string()
        } else if name.contains("_weekly_") {
            "weekly".to_string()
        } else if name.contains("_monthly_") {
            "monthly".to_string()
        } else if name.contains("_yearly_") {
            "yearly".to_string()
        } else {
            "daily".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_level() {
        assert_eq!(
            RetentionManager::detect_level("db_prod_hourly_2026-09-18.sql.zst"),
            "hourly"
        );
        assert_eq!(
            RetentionManager::detect_level("db_prod_daily_2026-09-18.sql.zst"),
            "daily"
        );
        assert_eq!(
            RetentionManager::detect_level("fs_app_weekly_2026-09-18.tar.zst"),
            "weekly"
        );
        assert_eq!(
            RetentionManager::detect_level("db_prod_monthly_2026-09-18.sql.zst"),
            "monthly"
        );
        assert_eq!(
            RetentionManager::detect_level("db_prod_yearly_2026-09-18.sql.zst"),
            "yearly"
        );
    }
    #[tokio::test]
    async fn retention_keeps_each_source_and_ignores_unmanaged_files() {
        let root = std::env::temp_dir().join(format!("titan-retention-{}", uuid::Uuid::new_v4()));
        let mut cfg = crate::config::Config::default();
        cfg.targets.local = Some(crate::config::LocalTargetConfig {
            enabled: true,
            path: root.clone(),
        });
        cfg.retention.keep_daily = 1;
        let storage = StorageManager::new(&cfg).unwrap();
        let target = storage.get_target("local").unwrap();
        for file in [
            "db/daily/db_one_daily_2026-01-01.sql.zst",
            "db/daily/db_one_daily_2026-01-02.sql.zst",
            "db/daily/db_two_daily_2026-01-01.sql.zst",
            "db/daily/db_two_daily_2026-01-02.sql.zst",
            "config/daily/fs_stack_daily_2026-01-01.tar.zst",
            "config/daily/fs_stack_daily_2026-01-02.tar.zst",
            "db/daily/historical.sql.bz2",
            "important.txt",
        ] {
            target.operator.write(file, b"test".to_vec()).await.unwrap();
        }
        assert_eq!(storage.list_snapshots("local").await.unwrap().len(), 8);
        assert_eq!(
            RetentionManager::prune_target(&storage, "local", &cfg.retention)
                .await
                .unwrap(),
            3
        );
        let remaining = storage.list_snapshots("local").await.unwrap();
        assert_eq!(remaining.len(), 5);
        for origin in ["db_one", "db_two", "fs_stack"] {
            assert_eq!(
                remaining
                    .iter()
                    .filter(|s| s.name.starts_with(origin))
                    .count(),
                1
            );
        }
        assert_eq!(
            storage.download("local", "important.txt").await.unwrap(),
            b"test"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
