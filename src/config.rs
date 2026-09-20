use sb_agent_core::config::{default_config_path, load, ConfigError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const AGENT_NAME: &str = "titanvault";
pub const BIN_NAME: &str = "titanvault";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default = "default_agent_name")]
    pub agent_name: String,
    #[serde(default = "default_mode")]
    pub mode: String, // "standalone" or "securyblack"
    #[serde(default)]
    pub cloud: CloudConfig,
    #[serde(default)]
    pub schedule: ScheduleConfig,
    #[serde(default)]
    pub crypto: CryptoConfig,
    #[serde(default)]
    pub retention: RetentionConfig,
    #[serde(default)]
    pub sources: SourcesConfig,
    #[serde(default)]
    pub targets: TargetsConfig,
}

fn default_version() -> String {
    VERSION.to_string()
}

fn default_agent_name() -> String {
    AGENT_NAME.to_string()
}

fn default_mode() -> String {
    "standalone".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudConfig {
    pub endpoint: Option<String>,
    pub token: Option<String>,
    #[serde(default)]
    pub heartbeat_url: Option<String>,
}

impl Default for CloudConfig {
    fn default() -> Self {
        Self {
            endpoint: None,
            token: None,
            heartbeat_url: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_cron")]
    pub cron: String, // e.g. "0 2 * * *" (daily at 02:00)
    #[serde(default = "default_hourly_cron")]
    pub hourly_cron: String, // e.g. "0 * * * *"
}

fn default_true() -> bool {
    true
}

fn default_cron() -> String {
    "0 2 * * *".to_string()
}

fn default_hourly_cron() -> String {
    "0 * * * *".to_string()
}

impl Default for ScheduleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cron: default_cron(),
            hourly_cron: default_hourly_cron(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryptoConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_algo")]
    pub algorithm: String, // "chacha20-poly1305"
    pub passphrase: Option<String>,
    pub key_file: Option<PathBuf>,
}

fn default_algo() -> String {
    "chacha20-poly1305".to_string()
}

impl Default for CryptoConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            algorithm: default_algo(),
            passphrase: None,
            key_file: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionConfig {
    #[serde(default = "default_keep_hourly")]
    pub keep_hourly: usize,
    #[serde(default = "default_keep_daily")]
    pub keep_daily: usize,
    #[serde(default = "default_keep_weekly")]
    pub keep_weekly: usize,
    #[serde(default = "default_keep_monthly")]
    pub keep_monthly: usize,
    #[serde(default = "default_keep_yearly")]
    pub keep_yearly: usize,
}

fn default_keep_hourly() -> usize {
    24
}
fn default_keep_daily() -> usize {
    7
}
fn default_keep_weekly() -> usize {
    4
}
fn default_keep_monthly() -> usize {
    12
}
fn default_keep_yearly() -> usize {
    3
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            keep_hourly: default_keep_hourly(),
            keep_daily: default_keep_daily(),
            keep_weekly: default_keep_weekly(),
            keep_monthly: default_keep_monthly(),
            keep_yearly: default_keep_yearly(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourcesConfig {
    #[serde(default)]
    pub databases: Vec<DatabaseSource>,
    #[serde(default)]
    pub filesystems: Vec<FilesystemSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseSource {
    pub name: String,
    #[serde(default = "default_db_driver")]
    pub driver: String, // "postgres", "mysql", "sqlite", "mongodb"
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub container_name: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub database: String,
    pub user: Option<String>,
    pub password: Option<String>,
}

fn default_db_driver() -> String {
    "postgres".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemSource {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub paths: Vec<PathBuf>,
    #[serde(default)]
    pub excludes: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TargetsConfig {
    #[serde(default)]
    pub hetzner: Option<S3TargetConfig>,
    #[serde(default)]
    pub cloudflare_r2: Option<S3TargetConfig>,
    #[serde(default)]
    pub generic_s3: Option<S3TargetConfig>,
    #[serde(default)]
    pub google_drive: Option<GDriveTargetConfig>,
    #[serde(default)]
    pub local: Option<LocalTargetConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3TargetConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub endpoint: String,
    pub bucket: String,
    #[serde(default = "default_region")]
    pub region: String,
    pub access_key: String,
    pub secret_key: String,
    #[serde(default = "default_prefix")]
    pub prefix: String,
}

fn default_region() -> String {
    "auto".to_string()
}

fn default_prefix() -> String {
    "backups".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GDriveTargetConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub remote_path: String, // e.g. "backups/securyblack"
    pub service_account_path: Option<PathBuf>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub refresh_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalTargetConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub path: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: default_version(),
            agent_name: default_agent_name(),
            mode: default_mode(),
            cloud: CloudConfig::default(),
            schedule: ScheduleConfig::default(),
            crypto: CryptoConfig::default(),
            retention: RetentionConfig::default(),
            sources: SourcesConfig::default(),
            targets: TargetsConfig::default(),
        }
    }
}

impl Config {
    pub fn load_default() -> Result<Self, ConfigError> {
        let path = default_config_path(AGENT_NAME);
        load::<Self>(&path)
    }

    pub fn load_from<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        load::<Self>(path.as_ref())
    }

    pub fn save_to<P: AsRef<Path>>(&self, path: P) -> std::io::Result<()> {
        let p = path.as_ref();
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let toml_str = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(p, toml_str)
    }

    pub fn save_default(&self) -> std::io::Result<()> {
        let path = default_config_path(AGENT_NAME);
        self.save_to(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_serialization() {
        let config = Config::default();
        let toml_str = toml::to_string_pretty(&config).expect("failed to serialize default config");
        assert!(toml_str.contains("titanvault"));
        assert!(toml_str.contains("standalone"));

        let deserialized: Config = toml::from_str(&toml_str).expect("failed to deserialize config");
        assert_eq!(deserialized.agent_name, "titanvault");
        assert_eq!(deserialized.retention.keep_daily, 7);
        assert_eq!(deserialized.retention.keep_hourly, 24);
    }
}

