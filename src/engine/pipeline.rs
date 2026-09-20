use crate::config::Config;
use crate::engine::crypto::CryptoEngine;
use crate::engine::dumper::{FilesDumper, PostgresDumper};
use crate::storage::StorageManager;
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupReport {
    pub job_id: String,
    pub source_name: String,
    pub source_type: String, // "database" | "filesystem"
    pub level: String,       // "hourly" | "daily" | "weekly" | "monthly"
    pub file_name: String,
    pub raw_bytes: usize,
    pub final_bytes: usize,
    pub compression_ratio: f64,
    pub encrypted: bool,
    pub duration_secs: f64,
    pub target_results: std::collections::HashMap<String, bool>,
    pub success: bool,
}

pub struct BackupPipeline {
    config: Config,
    storage: Arc<StorageManager>,
    crypto: Option<CryptoEngine>,
}

impl BackupPipeline {
    pub fn new(config: Config) -> Result<Self> {
        let storage = Arc::new(StorageManager::new(&config)?);
        let crypto = if config.crypto.enabled {
            if let Some(passphrase) = &config.crypto.passphrase {
                Some(CryptoEngine::from_passphrase(passphrase))
            } else if let Some(key_file) = &config.crypto.key_file {
                let pass = std::fs::read_to_string(key_file)
                    .context("failed to read crypto passphrase from key_file")?;
                Some(CryptoEngine::from_passphrase(pass.trim()))
            } else {
                warn!("Crypto enabled but no passphrase or key_file provided; disabling crypto");
                None
            }
        } else {
            None
        };

        Ok(Self {
            config,
            storage,
            crypto,
        })
    }

    pub fn storage(&self) -> Arc<StorageManager> {
        self.storage.clone()
    }

    /// Ejecuta el pipeline completo de backup para todos los orígenes activos
    pub async fn run_all(&self, level: &str) -> Vec<BackupReport> {
        let mut reports = Vec::new();

        // 1. Orígenes de Base de Datos
        for db in &self.config.sources.databases {
            if !db.enabled {
                continue;
            }
            match self.run_database_backup(db, level).await {
                Ok(report) => reports.push(report),
                Err(e) => {
                    error!("Database backup failed for '{}': {:#}", db.name, e);
                }
            }
        }

        // 2. Orígenes de Filesystem
        for fs in &self.config.sources.filesystems {
            if !fs.enabled {
                continue;
            }
            match self.run_filesystem_backup(fs, level).await {
                Ok(report) => reports.push(report),
                Err(e) => {
                    error!("Filesystem backup failed for '{}': {:#}", fs.name, e);
                }
            }
        }

        // 3. Heartbeat opcional a SecuryBlack Cloud
        if let Some(hb_url) = &self.config.cloud.heartbeat_url {
            let all_success = reports.iter().all(|r| r.success);
            if all_success && !reports.is_empty() {
                Self::ping_heartbeat(hb_url).await;
            }
        }

        reports
    }

    pub async fn run_database_backup(
        &self,
        source: &crate::config::DatabaseSource,
        level: &str,
    ) -> Result<BackupReport> {
        let start = Instant::now();
        let job_id = uuid::Uuid::new_v4().to_string();
        info!("Starting database backup job {} for '{}' (level: {})...", job_id, source.name, level);

        // 1. Dump (streaming)
        let raw_data = match source.driver.as_str() {
            "postgres" => PostgresDumper::dump(source).await?,
            other => return Err(anyhow!("unsupported database driver: {other}")),
        };
        let raw_bytes = raw_data.len();

        // 2. Compresión zstd (nivel 3: equilibrio óptimo CPU/ratio)
        let compressed_data = zstd::encode_all(&raw_data[..], 3)
            .context("failed to compress database dump with zstd")?;

        // 3. Cifrado simétrico Zero-Knowledge si está activo
        let (final_data, is_encrypted, ext) = if let Some(crypto) = &self.crypto {
            let enc = crypto.encrypt(&compressed_data)?;
            (enc, true, "sql.zst.enc")
        } else {
            (compressed_data, false, "sql.zst")
        };
        let final_bytes = final_data.len();

        // 4. Nombre normalizado del archivo
        let date_tag = Utc::now().format("%Y-%m-%d_%H-%M-%S");
        let file_name = format!("db_{}_{}_{}.{}", source.name, level, date_tag, ext);
        let rel_path = format!("db/{}/{}", level, file_name);

        // 5. Subida concurrente a todos los destinos configurados
        let upload_results = self.storage.upload_all(&rel_path, Arc::new(final_data)).await;
        let mut target_results = std::collections::HashMap::new();
        let mut all_ok = true;

        for (target_id, res) in upload_results {
            match res {
                Ok(_) => {
                    info!("Uploaded '{}' to target '{}' [OK]", file_name, target_id);
                    target_results.insert(target_id, true);
                }
                Err(e) => {
                    error!("Failed to upload '{}' to target '{}': {:#}", file_name, target_id, e);
                    target_results.insert(target_id, false);
                    all_ok = false;
                }
            }
        }

        let duration_secs = start.elapsed().as_secs_f64();
        let ratio = if raw_bytes > 0 {
            (1.0 - (final_bytes as f64 / raw_bytes as f64)) * 100.0
        } else {
            0.0
        };

        info!(
            "Backup job {} finished in {:.2}s: {} bytes -> {} bytes ({:.1}% saved)",
            job_id, duration_secs, raw_bytes, final_bytes, ratio
        );

        Ok(BackupReport {
            job_id,
            source_name: source.name.clone(),
            source_type: "database".to_string(),
            level: level.to_string(),
            file_name,
            raw_bytes,
            final_bytes,
            compression_ratio: ratio,
            encrypted: is_encrypted,
            duration_secs,
            target_results,
            success: all_ok,
        })
    }

    pub async fn run_filesystem_backup(
        &self,
        source: &crate::config::FilesystemSource,
        level: &str,
    ) -> Result<BackupReport> {
        let start = Instant::now();
        let job_id = uuid::Uuid::new_v4().to_string();
        info!("Starting filesystem backup job {} for '{}' (level: {})...", job_id, source.name, level);

        // 1. Tar dump (streaming)
        let raw_data = FilesDumper::dump(source).await?;
        let raw_bytes = raw_data.len();

        // 2. Compresión zstd
        let compressed_data = zstd::encode_all(&raw_data[..], 3)
            .context("failed to compress filesystem tar with zstd")?;

        // 3. Cifrado simétrico
        let (final_data, is_encrypted, ext) = if let Some(crypto) = &self.crypto {
            let enc = crypto.encrypt(&compressed_data)?;
            (enc, true, "tar.zst.enc")
        } else {
            (compressed_data, false, "tar.zst")
        };
        let final_bytes = final_data.len();

        // 4. Nombre normalizado
        let date_tag = Utc::now().format("%Y-%m-%d_%H-%M-%S");
        let file_name = format!("fs_{}_{}_{}.{}", source.name, level, date_tag, ext);
        let rel_path = format!("config/{}/{}", level, file_name);

        // 5. Subida concurrente
        let upload_results = self.storage.upload_all(&rel_path, Arc::new(final_data)).await;
        let mut target_results = std::collections::HashMap::new();
        let mut all_ok = true;

        for (target_id, res) in upload_results {
            match res {
                Ok(_) => {
                    info!("Uploaded '{}' to target '{}' [OK]", file_name, target_id);
                    target_results.insert(target_id, true);
                }
                Err(e) => {
                    error!("Failed to upload '{}' to target '{}': {:#}", file_name, target_id, e);
                    target_results.insert(target_id, false);
                    all_ok = false;
                }
            }
        }

        let duration_secs = start.elapsed().as_secs_f64();
        let ratio = if raw_bytes > 0 {
            (1.0 - (final_bytes as f64 / raw_bytes as f64)) * 100.0
        } else {
            0.0
        };

        Ok(BackupReport {
            job_id,
            source_name: source.name.clone(),
            source_type: "filesystem".to_string(),
            level: level.to_string(),
            file_name,
            raw_bytes,
            final_bytes,
            compression_ratio: ratio,
            encrypted: is_encrypted,
            duration_secs,
            target_results,
            success: all_ok,
        })
    }

    /// Envía un ping HTTP al monitor de heartbeat de SecuryBlack
    async fn ping_heartbeat(url: &str) {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build();

        if let Ok(c) = client {
            match c.post(url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    info!("SecuryBlack heartbeat ping sent successfully");
                }
                Ok(resp) => {
                    warn!("SecuryBlack heartbeat ping returned status: {}", resp.status());
                }
                Err(e) => {
                    warn!("Could not send SecuryBlack heartbeat ping: {:#}", e);
                }
            }
        }
    }
}
