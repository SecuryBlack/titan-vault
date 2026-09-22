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
        // 1. Guardar a disco
        new_cfg
            .save_to(&self.config_path)
            .map_err(|e| format!("Failed to save config to {}: {e}", self.config_path.display()))?;

        // 2. Inicializar nuevo pipeline
        let new_pipeline = BackupPipeline::new(new_cfg.clone())
            .map_err(|e| format!("Failed to initialize backup pipeline with new config: {e:#}"))?;
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
