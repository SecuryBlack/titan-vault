use crate::config::Config;
use crate::engine::pipeline::BackupPipeline;
use sb_agent_core::status::StatusHandle;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct SharedVaultState {
    pub config: RwLock<Config>,
    pub pipeline: RwLock<Arc<BackupPipeline>>,
    pub config_path: PathBuf,
    pub status_handle: Option<StatusHandle>,
}

impl SharedVaultState {
    pub fn new(
        config: Config,
        pipeline: Arc<BackupPipeline>,
        config_path: PathBuf,
        status_handle: Option<StatusHandle>,
    ) -> Self {
        Self {
            config: RwLock::new(config),
            pipeline: RwLock::new(pipeline),
            config_path,
            status_handle,
        }
    }

    pub async fn apply_new_config(&self, new_cfg: Config) -> Result<(), String> {
        // Validate before persisting: invalid encryption settings must not replace a working config.
        let new_pipeline = BackupPipeline::new(new_cfg.clone())
            .map_err(|e| format!("Failed to initialize backup pipeline with new config: {e:#}"))?;
        new_cfg.save_to(&self.config_path).map_err(|e| {
            format!(
                "Failed to save config to {}: {e}",
                self.config_path.display()
            )
        })?;

        let new_pipeline_arc = Arc::new(new_pipeline);

        // 3. Actualizar estado en memoria
        {
            let mut cfg_lock = self.config.write().await;
            *cfg_lock = new_cfg.clone();
        }
        {
            let mut pipe_lock = self.pipeline.write().await;
            *pipe_lock = new_pipeline_arc.clone();
        }

        // 4. Actualizar status socket
        if let Some(status) = &self.status_handle {
            status.set_details(serde_json::json!({
                "mode": new_cfg.mode,
                "databases_count": new_cfg.sources.databases.len(),
                "filesystems_count": new_cfg.sources.filesystems.len(),
                "targets_count": new_pipeline_arc.storage().get_targets().len(),
                "schedule_enabled": new_cfg.schedule.enabled,
            }));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn invalid_crypto_update_does_not_replace_saved_or_running_config() {
        let dir =
            std::env::temp_dir().join(format!("titanvault-state-test-{}", uuid::Uuid::new_v4()));
        let path = dir.join("config.toml");
        let cfg = Config::default();
        cfg.save_to(&path).unwrap();
        let original = std::fs::read(&path).unwrap();
        let pipeline = Arc::new(BackupPipeline::new(cfg.clone()).unwrap());
        let state = SharedVaultState::new(cfg.clone(), pipeline.clone(), path.clone(), None);
        let mut invalid = cfg;
        invalid.crypto.enabled = true;
        assert!(state.apply_new_config(invalid).await.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(!state.config.read().await.crypto.enabled);
        assert!(Arc::ptr_eq(&pipeline, &*state.pipeline.read().await));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
