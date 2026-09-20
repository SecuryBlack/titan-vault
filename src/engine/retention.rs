use crate::config::RetentionConfig;
use crate::storage::{SnapshotMeta, StorageManager};
use anyhow::Result;
use std::collections::HashMap;
use tracing::{info, warn};

pub struct RetentionManager;

impl RetentionManager {
    /// Aplica la rotación GFS (Grandfather-Father-Son) en todos los destinos de almacenamiento
    pub async fn prune_target(
        storage: &StorageManager,
        target_id: &str,
        retention: &RetentionConfig,
    ) -> Result<usize> {
        info!("Running GFS retention pruning on target '{}'...", target_id);
        let snapshots = storage.list_snapshots(target_id).await?;

        // Agrupar por nivel (hourly, daily, weekly, monthly, yearly)
        let mut by_level: HashMap<String, Vec<SnapshotMeta>> = HashMap::new();

        for snap in snapshots {
            let level = Self::detect_level(&snap.name);
            by_level.entry(level).or_default().push(snap);
        }

        let mut total_deleted = 0;

        for (level, mut snaps) in by_level {
            // Ordenar por fecha descendente (los más recientes primero)
            snaps.sort_by(|a, b| b.created_at.cmp(&a.created_at));

            let keep_limit = match level.as_str() {
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
                    if let Err(e) = storage.delete(target_id, &snap.path).await {
                        warn!("Failed to delete snapshot '{}': {:#}", snap.name, e);
                    } else {
                        total_deleted += 1;
                    }
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
        assert_eq!(RetentionManager::detect_level("db_prod_hourly_2026-09-18.sql.zst"), "hourly");
        assert_eq!(RetentionManager::detect_level("db_prod_daily_2026-09-18.sql.zst"), "daily");
        assert_eq!(RetentionManager::detect_level("fs_app_weekly_2026-09-18.tar.zst"), "weekly");
        assert_eq!(RetentionManager::detect_level("db_prod_monthly_2026-09-18.sql.zst"), "monthly");
        assert_eq!(RetentionManager::detect_level("db_prod_yearly_2026-09-18.sql.zst"), "yearly");
    }
}

