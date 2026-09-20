use crate::config::{Config, S3TargetConfig};
use anyhow::{anyhow, Context, Result};
use opendal::{services, Operator};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMeta {
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub target_id: String,
}

#[derive(Clone)]
pub struct StorageTarget {
    pub id: String,
    pub name: String,
    pub operator: Operator,
    pub prefix: String,
}

pub struct StorageManager {
    targets: HashMap<String, StorageTarget>,
}

impl StorageManager {
    pub fn new(config: &Config) -> Result<Self> {
        let mut targets = HashMap::new();

        // 1. Hetzner Object Storage (S3-compatible)
        if let Some(hetzner) = &config.targets.hetzner {
            if hetzner.enabled {
                let op = Self::build_s3_operator(hetzner)?;
                targets.insert(
                    "hetzner".to_string(),
                    StorageTarget {
                        id: "hetzner".to_string(),
                        name: "Hetzner Object Storage".to_string(),
                        operator: op,
                        prefix: hetzner.prefix.clone(),
                    },
                );
            }
        }

        // 2. Cloudflare R2 (S3-compatible)
        if let Some(r2) = &config.targets.cloudflare_r2 {
            if r2.enabled {
                let op = Self::build_s3_operator(r2)?;
                targets.insert(
                    "cloudflare_r2".to_string(),
                    StorageTarget {
                        id: "cloudflare_r2".to_string(),
                        name: "Cloudflare R2".to_string(),
                        operator: op,
                        prefix: r2.prefix.clone(),
                    },
                );
            }
        }

        // 3. Generic S3 / MinIO / AWS
        if let Some(s3) = &config.targets.generic_s3 {
            if s3.enabled {
                let op = Self::build_s3_operator(s3)?;
                targets.insert(
                    "generic_s3".to_string(),
                    StorageTarget {
                        id: "generic_s3".to_string(),
                        name: "S3 Compatible".to_string(),
                        operator: op,
                        prefix: s3.prefix.clone(),
                    },
                );
            }
        }

        // 4. Local Storage / NFS
        if let Some(local) = &config.targets.local {
            if local.enabled {
                let mut builder = services::Fs::default();
                let root_str = local.path.to_string_lossy().to_string();
                builder = builder.root(&root_str);
                let op = Operator::new(builder)
                    .context("failed to build local fs storage operator")?;

                targets.insert(
                    "local".to_string(),
                    StorageTarget {
                        id: "local".to_string(),
                        name: "Local Storage".to_string(),
                        operator: op,
                        prefix: "".to_string(),
                    },
                );
            }
        }

        Ok(Self { targets })
    }

    fn build_s3_operator(s3: &S3TargetConfig) -> Result<Operator> {
        let mut builder = services::S3::default();
        builder = builder
            .bucket(&s3.bucket)
            .endpoint(&s3.endpoint)
            .region(&s3.region)
            .access_key_id(&s3.access_key)
            .secret_access_key(&s3.secret_key);

        let op = Operator::new(builder)
            .context(format!("failed to build S3 operator for bucket {}", s3.bucket))?;

        Ok(op)
    }

    pub fn get_targets(&self) -> Vec<StorageTarget> {
        self.targets.values().cloned().collect()
    }

    pub fn get_target(&self, id: &str) -> Option<&StorageTarget> {
        self.targets.get(id)
    }

    /// Comprueba la conectividad subiendo y eliminando un archivo .healthcheck efímero
    pub async fn test_connection(&self, target_id: &str) -> Result<()> {
        let target = self
            .targets
            .get(target_id)
            .ok_or_else(|| anyhow!("storage target '{}' not configured or disabled", target_id))?;

        let check_path = if target.prefix.is_empty() {
            ".titanvault_healthcheck".to_string()
        } else {
            format!("{}/.titanvault_healthcheck", target.prefix.trim_end_matches('/'))
        };

        let dummy_payload = format!("healthcheck:{}", chrono::Utc::now());
        target
            .operator
            .write(&check_path, dummy_payload.as_bytes().to_vec())
            .await
            .context(format!("write test failed on {}", target.name))?;

        let _ = target.operator.delete(&check_path).await;
        Ok(())
    }

    /// Sube datos a todos los destinos configurados en paralelo
    pub async fn upload_all(&self, rel_path: &str, data: Arc<Vec<u8>>) -> HashMap<String, Result<()>> {
        let mut results = HashMap::new();

        for (id, target) in &self.targets {
            let full_path = if target.prefix.is_empty() {
                rel_path.to_string()
            } else {
                format!("{}/{}", target.prefix.trim_end_matches('/'), rel_path.trim_start_matches('/'))
            };

            let op = target.operator.clone();
            let buf = data.clone();
            let res = op
                .write(&full_path, (*buf).clone())
                .await
                .map(|_| ())
                .map_err(|e| anyhow!("{e}"));
            results.insert(id.clone(), res);
        }

        results
    }

    /// Lista snapshots disponibles en un destino
    pub async fn list_snapshots(&self, target_id: &str) -> Result<Vec<SnapshotMeta>> {
        use futures_util::StreamExt;

        let target = self
            .targets
            .get(target_id)
            .ok_or_else(|| anyhow!("storage target '{}' not found", target_id))?;

        let prefix = if target.prefix.is_empty() {
            "".to_string()
        } else {
            format!("{}/", target.prefix.trim_matches('/'))
        };

        let mut lister = target.operator.lister_with(&prefix).await?;
        let mut snapshots = Vec::new();

        while let Some(entry) = lister.next().await {
            let entry = entry?;
            let meta = target.operator.stat(entry.path()).await?;
            if meta.is_file() {
                let name = entry.name().to_string();
                let path = entry.path().to_string();
                let size = meta.content_length();
                let modified = meta
                    .last_modified()
                    .map(|t| {
                        let st: std::time::SystemTime = t.into();
                        chrono::DateTime::<chrono::Utc>::from(st)
                    })
                    .unwrap_or_else(chrono::Utc::now);

                snapshots.push(SnapshotMeta {
                    name,
                    path,
                    size_bytes: size,
                    created_at: modified,
                    target_id: target_id.to_string(),
                });
            }
        }

        snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(snapshots)
    }

    /// Descarga un snapshot desde el destino especificado
    pub async fn download(&self, target_id: &str, rel_path: &str) -> Result<Vec<u8>> {
        let target = self
            .targets
            .get(target_id)
            .ok_or_else(|| anyhow!("storage target '{}' not found", target_id))?;

        let full_path = if target.prefix.is_empty() {
            rel_path.to_string()
        } else {
            format!("{}/{}", target.prefix.trim_end_matches('/'), rel_path.trim_start_matches('/'))
        };

        let bytes = target.operator.read(&full_path).await?;
        Ok(bytes.to_vec())
    }

    /// Elimina un snapshot remoto
    pub async fn delete(&self, target_id: &str, path: &str) -> Result<()> {
        let target = self
            .targets
            .get(target_id)
            .ok_or_else(|| anyhow!("storage target '{}' not found", target_id))?;

        target.operator.delete(path).await?;
        Ok(())
    }
}
