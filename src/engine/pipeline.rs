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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub struct BackupPipeline {
    config: Config,
    storage: Arc<StorageManager>,
    crypto: Option<CryptoEngine>,
    execution_lock: tokio::sync::Mutex<()>,
}

impl BackupPipeline {
    pub fn new(config: Config) -> Result<Self> {
        if config.schedule.enabled {
            config.schedule.validate()?;
        }
        let storage = Arc::new(StorageManager::new(&config)?);
        let crypto = if config.crypto.enabled {
            if let Some(passphrase) = &config.crypto.passphrase {
                if passphrase.trim().is_empty() {
                    return Err(anyhow!("encryption is enabled but passphrase is empty"));
                }
                Some(CryptoEngine::from_passphrase(passphrase))
            } else if let Some(key_file) = &config.crypto.key_file {
                let pass = std::fs::read_to_string(key_file)
                    .context("failed to read crypto passphrase from key_file")?;
                if pass.trim().is_empty() {
                    return Err(anyhow!("encryption is enabled but key_file is empty"));
                }
                Some(CryptoEngine::from_passphrase(pass.trim()))
            } else {
                return Err(anyhow!(
                    "encryption is enabled but no passphrase or key_file is configured"
                ));
            }
        } else {
            None
        };

        Ok(Self {
            config,
            storage,
            crypto,
            execution_lock: tokio::sync::Mutex::new(()),
        })
    }

    fn failed_report(
        &self,
        source_name: &str,
        source_type: &str,
        level: &str,
        err: &anyhow::Error,
    ) -> BackupReport {
        error!("Backup failed for '{}': {:#}", source_name, err);
        BackupReport {
            job_id: uuid::Uuid::new_v4().to_string(),
            source_name: source_name.to_string(),
            source_type: source_type.to_string(),
            level: level.to_string(),
            file_name: String::new(),
            raw_bytes: 0,
            final_bytes: 0,
            compression_ratio: 0.0,
            encrypted: false,
            duration_secs: 0.0,
            target_results: Default::default(),
            success: false,
            error: Some(format!("{err:#}")),
        }
    }

    pub fn storage(&self) -> Arc<StorageManager> {
        self.storage.clone()
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Ejecuta el pipeline para un único origen específico por su nombre
    pub async fn run_source(&self, source_name: &str, level: &str) -> Vec<BackupReport> {
        let _guard = self.execution_lock.lock().await;
        let mut reports = Vec::new();

        for db in &self.config.sources.databases {
            if db.name == source_name && db.enabled {
                match self.run_database_backup(db, level).await {
                    Ok(report) => reports.push(report),
                    Err(e) => reports.push(self.failed_report(&db.name, "database", level, &e)),
                }
                return reports;
            }
        }

        for fs in &self.config.sources.filesystems {
            if fs.name == source_name && fs.enabled {
                match self.run_filesystem_backup(fs, level).await {
                    Ok(report) => reports.push(report),
                    Err(e) => reports.push(self.failed_report(&fs.name, "filesystem", level, &e)),
                }
                return reports;
            }
        }

        reports
    }

    /// Ejecuta el pipeline completo de backup para todos los orígenes activos
    pub async fn run_all(&self, level: &str) -> Vec<BackupReport> {
        let _guard = self.execution_lock.lock().await;
        let mut reports = Vec::new();

        // 1. Orígenes de Base de Datos
        for db in &self.config.sources.databases {
            if !db.enabled {
                continue;
            }
            match self.run_database_backup(db, level).await {
                Ok(report) => reports.push(report),
                Err(e) => {
                    reports.push(self.failed_report(&db.name, "database", level, &e));
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
                    reports.push(self.failed_report(&fs.name, "filesystem", level, &e));
                }
            }
        }

        // 3. Heartbeat opcional a SecuryBlack Cloud
        if let Some(hb_url) = self.config.cloud.heartbeat_for_level(level) {
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
        if !["hourly", "daily", "weekly", "monthly", "yearly"].contains(&level) {
            return Err(anyhow!("invalid backup level: {level}"));
        }
        if source.name.trim().is_empty() || source.name.contains('/') || source.name.contains('\\')
        {
            return Err(anyhow!("source name must be a nonempty filename component"));
        }
        if self.storage.get_targets().is_empty() {
            return Err(anyhow!("no enabled backup storage targets are configured"));
        }
        let start = Instant::now();
        let job_id = uuid::Uuid::new_v4().to_string();
        info!(
            "Starting database backup job {} for '{}' (level: {})...",
            job_id, source.name, level
        );

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
        let upload_results = self
            .storage
            .upload_all(&rel_path, Arc::new(final_data))
            .await;
        let mut target_results = std::collections::HashMap::new();
        let mut all_ok = !upload_results.is_empty();

        for (target_id, res) in upload_results {
            match res {
                Ok(_) => {
                    info!("Uploaded '{}' to target '{}' [OK]", file_name, target_id);
                    target_results.insert(target_id, true);
                }
                Err(e) => {
                    error!(
                        "Failed to upload '{}' to target '{}': {:#}",
                        file_name, target_id, e
                    );
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
            error: if all_ok {
                None
            } else {
                Some("one or more storage uploads failed".to_string())
            },
        })
    }

    pub async fn run_filesystem_backup(
        &self,
        source: &crate::config::FilesystemSource,
        level: &str,
    ) -> Result<BackupReport> {
        if !["hourly", "daily", "weekly", "monthly", "yearly"].contains(&level) {
            return Err(anyhow!("invalid backup level: {level}"));
        }
        if source.name.trim().is_empty() || source.name.contains('/') || source.name.contains('\\')
        {
            return Err(anyhow!("source name must be a nonempty filename component"));
        }
        if self.storage.get_targets().is_empty() {
            return Err(anyhow!("no enabled backup storage targets are configured"));
        }
        let start = Instant::now();
        let job_id = uuid::Uuid::new_v4().to_string();
        info!(
            "Starting filesystem backup job {} for '{}' (level: {})...",
            job_id, source.name, level
        );

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
        let upload_results = self
            .storage
            .upload_all(&rel_path, Arc::new(final_data))
            .await;
        let mut target_results = std::collections::HashMap::new();
        let mut all_ok = !upload_results.is_empty();

        for (target_id, res) in upload_results {
            match res {
                Ok(_) => {
                    info!("Uploaded '{}' to target '{}' [OK]", file_name, target_id);
                    target_results.insert(target_id, true);
                }
                Err(e) => {
                    error!(
                        "Failed to upload '{}' to target '{}': {:#}",
                        file_name, target_id, e
                    );
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
            error: if all_ok {
                None
            } else {
                Some("one or more storage uploads failed".to_string())
            },
        })
    }

    /// Envía un ping HTTP al monitor de heartbeat de SecuryBlack
    async fn ping_heartbeat(url: &str) {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(10))
            .build();

        if let Ok(c) = client {
            match c.post(url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    info!("SecuryBlack heartbeat ping sent successfully");
                }
                Ok(resp) => {
                    warn!(
                        "SecuryBlack heartbeat ping returned status: {}",
                        resp.status()
                    );
                }
                Err(e) => {
                    warn!("Could not send SecuryBlack heartbeat ping: {:#}", e);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DatabaseSource, FilesystemSource, LocalTargetConfig};

    struct TestDir(std::path::PathBuf);
    impl TestDir {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("titanvault-test-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn source(name: &str, path: std::path::PathBuf) -> FilesystemSource {
        FilesystemSource {
            name: name.into(),
            enabled: true,
            paths: vec![path],
            excludes: vec![],
        }
    }
    fn local_config(dir: &TestDir) -> Config {
        let mut cfg = Config::default();
        cfg.targets.local = Some(LocalTargetConfig {
            enabled: true,
            path: dir.0.join("target"),
        });
        cfg
    }

    #[test]
    fn enabled_crypto_rejects_missing_or_blank_keys() {
        let mut cfg = Config::default();
        cfg.crypto.enabled = true;
        assert!(BackupPipeline::new(cfg.clone()).is_err());
        cfg.crypto.passphrase = Some("   ".into());
        assert!(BackupPipeline::new(cfg.clone()).is_err());
        cfg.crypto.passphrase = Some("valid-key".into());
        assert!(BackupPipeline::new(cfg).is_ok());
    }

    #[test]
    fn enabled_crypto_rejects_empty_or_missing_key_file() {
        let dir = TestDir::new();
        let key = dir.0.join("key");
        let mut cfg = Config::default();
        cfg.crypto.enabled = true;
        cfg.crypto.key_file = Some(key.clone());
        assert!(BackupPipeline::new(cfg.clone()).is_err());
        std::fs::write(&key, " \n").unwrap();
        assert!(BackupPipeline::new(cfg.clone()).is_err());
        std::fs::write(&key, "valid-key\n").unwrap();
        assert!(BackupPipeline::new(cfg).is_ok());
    }

    #[tokio::test]
    async fn missing_target_produces_failure_for_manual_and_full_backup() {
        let dir = TestDir::new();
        let mut cfg = Config::default();
        cfg.sources.filesystems.push(source("files", dir.0.clone()));
        let p = BackupPipeline::new(cfg).unwrap();
        for reports in [
            p.run_all("daily").await,
            p.run_source("files", "daily").await,
        ] {
            assert_eq!(reports.len(), 1);
            assert!(!reports[0].success);
            assert!(reports[0]
                .error
                .as_ref()
                .unwrap()
                .contains("no enabled backup storage targets"));
        }
    }

    #[tokio::test]
    async fn mixed_backup_keeps_failed_source_and_does_not_ping_success() {
        let dir = TestDir::new();
        let input = dir.0.join("input.txt");
        std::fs::write(&input, "backup payload").unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut cfg = local_config(&dir);
        cfg.cloud.heartbeat_url = Some(format!(
            "http://{}/heartbeat",
            listener.local_addr().unwrap()
        ));
        cfg.sources.filesystems.push(source("files", input));
        cfg.sources.databases.push(DatabaseSource {
            name: "broken".into(),
            driver: "unsupported".into(),
            enabled: true,
            container_name: None,
            host: None,
            port: None,
            database: "db".into(),
            user: None,
            password: None,
        });
        let p = BackupPipeline::new(cfg).unwrap();
        let reports = p.run_all("daily").await;
        assert_eq!(reports.len(), 2);
        assert!(!reports[0].success);
        assert!(reports[0]
            .error
            .as_ref()
            .unwrap()
            .contains("unsupported database driver"));
        assert!(reports[1].success);
        let path = format!("config/daily/{}", reports[1].file_name);
        let compressed = p.storage.download("local", &path).await.unwrap();
        let tar = zstd::decode_all(&compressed[..]).unwrap();
        let mut archive = tar::Archive::new(&tar[..]);
        let mut entries = archive.entries().unwrap();
        let mut entry = entries.next().unwrap().unwrap();
        let mut text = String::new();
        std::io::Read::read_to_string(&mut entry, &mut text).unwrap();
        assert_eq!(text, "backup payload");
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), listener.accept())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn missing_files_are_reported_and_disabled_sources_are_skipped() {
        let dir = TestDir::new();
        let mut cfg = local_config(&dir);
        cfg.sources
            .filesystems
            .push(source("missing", dir.0.join("absent")));
        let p = BackupPipeline::new(cfg.clone()).unwrap();
        let reports = p.run_all("daily").await;
        assert_eq!(reports.len(), 1);
        assert!(!reports[0].success);
        assert!(reports[0]
            .error
            .as_ref()
            .unwrap()
            .contains("does not exist"));
        cfg.sources.filesystems[0].enabled = false;
        let p = BackupPipeline::new(cfg).unwrap();
        assert!(p.run_source("missing", "daily").await.is_empty());
    }
}
