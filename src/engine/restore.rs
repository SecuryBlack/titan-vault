//! Recovery never writes over an existing destination and never executes recovered SQL.
use crate::{config::Config, engine::crypto::CryptoEngine, storage::StorageManager};
use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use std::{
    collections::HashSet,
    io::{Cursor, Write},
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Serialize)]
pub struct RestoreReport {
    pub target_id: String,
    pub snapshot_path: String,
    pub destination: PathBuf,
    pub kind: String,
    pub recovered_bytes: usize,
    pub files: usize,
}

/// Recover a relative snapshot path from a configured target. Filesystem snapshots require
/// a new directory; SQL snapshots require a new file. SQL is exported for an isolated restore,
/// never run against the configured source database.
pub async fn restore_snapshot(
    config: &Config,
    storage: &StorageManager,
    target_id: &str,
    snapshot_path: &str,
    destination: &Path,
) -> Result<RestoreReport> {
    validate_relative_path(snapshot_path)?;
    let bytes = storage
        .download(target_id, snapshot_path)
        .await
        .context("download recovery snapshot")?;
    let config = config.clone();
    let name = snapshot_path.to_owned();
    let dest = destination.to_owned();
    let (kind, recovered_bytes, files) =
        tokio::task::spawn_blocking(move || recover_payload(&config, &name, &bytes, &dest))
            .await??;
    Ok(RestoreReport {
        target_id: target_id.into(),
        snapshot_path: snapshot_path.into(),
        destination: destination.into(),
        kind,
        recovered_bytes,
        files,
    })
}

fn validate_relative_path(name: &str) -> Result<()> {
    if name.is_empty()
        || name.contains('\\')
        || name.contains(':')
        || name.contains('\0')
        || !Path::new(name)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
        || name
            .split('/')
            .any(|p| p == "." || p == ".." || p.is_empty())
    {
        return Err(anyhow!("unsafe recovery path"));
    }
    Ok(())
}

fn recover_payload(
    config: &Config,
    name: &str,
    bytes: &[u8],
    destination: &Path,
) -> Result<(String, usize, usize)> {
    if std::fs::symlink_metadata(destination).is_ok() {
        return Err(anyhow!("recovery destination already exists"));
    }
    let (plain_name, compressed) = if let Some(base) = name.strip_suffix(".enc") {
        let passphrase = if let Some(pass) = &config.crypto.passphrase {
            pass.clone()
        } else if let Some(file) = &config.crypto.key_file {
            std::fs::read_to_string(file)?.trim().to_owned()
        } else {
            return Err(anyhow!(
                "encrypted snapshot requires passphrase or key_file"
            ));
        };
        if passphrase.trim().is_empty() {
            return Err(anyhow!("recovery key is empty"));
        }
        (
            base,
            CryptoEngine::from_passphrase(&passphrase).decrypt(bytes)?,
        )
    } else {
        (name, bytes.to_vec())
    };
    if !plain_name.ends_with(".tar.zst") && !plain_name.ends_with(".sql.zst") {
        return Err(anyhow!("unsupported snapshot format"));
    }
    let raw = zstd::decode_all(&compressed[..]).context("decompress recovery snapshot")?;
    if plain_name.ends_with(".sql.zst") {
        if raw.is_empty() {
            return Err(anyhow!("empty SQL snapshot"));
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(destination)
            .context("create new SQL recovery file")?;
        file.write_all(&raw)?;
        file.sync_all()?;
        return Ok(("sql_export".into(), raw.len(), 1));
    }
    // Validate every member before creating the destination, including payload completeness.
    let mut paths = HashSet::new();
    let mut files = 0;
    for entry in tar::Archive::new(Cursor::new(&raw)).entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let name = path
            .to_str()
            .ok_or_else(|| anyhow!("archive path must be UTF-8"))?;
        validate_relative_path(name.trim_end_matches('/'))?;
        let kind = entry.header().entry_type();
        if !kind.is_file() && !kind.is_dir() {
            return Err(anyhow!("archive links and special entries are not allowed"));
        }
        let key = if cfg!(windows) {
            name.trim_end_matches('/').to_lowercase()
        } else {
            name.trim_end_matches('/').to_owned()
        };
        if !paths.insert(key) {
            return Err(anyhow!("duplicate archive path"));
        }
        std::io::copy(&mut entry, &mut std::io::sink()).context("truncated archive entry")?;
        if kind.is_file() {
            files += 1;
        }
    }
    if files == 0 {
        return Err(anyhow!("archive contains no recoverable files"));
    }
    let directory = std::fs::DirBuilder::new();
    #[cfg(unix)]
    let directory = {
        use std::os::unix::fs::DirBuilderExt;
        let mut directory = directory;
        directory.mode(0o700);
        directory
    };
    directory
        .create(destination)
        .context("create new recovery directory")?;
    let extraction: Result<()> = (|| {
        let mut archive = tar::Archive::new(Cursor::new(&raw));
        archive.set_overwrite(false);
        for entry in archive.entries()? {
            let mut entry = entry?;
            if !entry.unpack_in(destination)? {
                return Err(anyhow!("archive member escaped recovery directory"));
            }
        }
        Ok(())
    })();
    extraction.context("extract validated recovery archive")?;
    Ok(("filesystem".into(), raw.len(), files))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp() -> PathBuf {
        std::env::temp_dir().join(format!("titanvault-recovery-{}", uuid::Uuid::new_v4()))
    }
    fn archive(path: &str, kind: tar::EntryType) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(5);
        header.set_mode(0o600);
        header.set_entry_type(kind);
        header.set_cksum();
        builder
            .append_data(&mut header, path, &b"hello"[..])
            .unwrap();
        zstd::encode_all(&builder.into_inner().unwrap()[..], 3).unwrap()
    }
    #[tokio::test]
    async fn encrypted_local_recovery_roundtrip() {
        let root = temp();
        std::fs::create_dir(&root).unwrap();
        let mut config = Config::default();
        config.targets.local = Some(crate::config::LocalTargetConfig {
            enabled: true,
            path: root.clone(),
        });
        config.crypto.passphrase = Some("test recovery password".into());
        let storage = StorageManager::new(&config).unwrap();
        let data = CryptoEngine::from_passphrase(config.crypto.passphrase.as_ref().unwrap())
            .encrypt(&archive("stack/config.txt", tar::EntryType::Regular))
            .unwrap();
        let results = storage
            .upload_all(
                "config/daily/snapshot.tar.zst.enc",
                std::sync::Arc::new(data),
            )
            .await;
        assert!(results.values().all(|r| r.is_ok()));
        let destination = root.join("recovered");
        let report = restore_snapshot(
            &config,
            &storage,
            "local",
            "config/daily/snapshot.tar.zst.enc",
            &destination,
        )
        .await
        .unwrap();
        assert_eq!(report.files, 1);
        assert_eq!(
            std::fs::read(destination.join("stack/config.txt")).unwrap(),
            b"hello"
        );
        assert!(restore_snapshot(
            &config,
            &storage,
            "local",
            "config/daily/snapshot.tar.zst.enc",
            &destination
        )
        .await
        .is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn wrong_key_and_corruption_do_not_create_destination() {
        let mut config = Config::default();
        config.crypto.passphrase = Some("wrong".into());
        let encrypted = CryptoEngine::from_passphrase("correct")
            .encrypt(&archive("file", tar::EntryType::Regular))
            .unwrap();
        let dest = temp();
        assert!(recover_payload(&config, "a.tar.zst.enc", &encrypted, &dest).is_err());
        assert!(recover_payload(&config, "a.tar.zst", b"broken", &dest).is_err());
        assert!(!dest.exists());
    }
    #[test]
    fn unsafe_paths_and_links_are_rejected() {
        for path in [
            "../outside",
            "/absolute",
            "C:/outside",
            "a\\..\\outside",
            "a/./file",
        ] {
            assert!(validate_relative_path(path).is_err());
        }
        let dest = temp();
        assert!(recover_payload(
            &Config::default(),
            "a.tar.zst",
            &archive("link", tar::EntryType::Symlink),
            &dest
        )
        .is_err());
        assert!(!dest.exists());
    }
    #[test]
    fn malicious_archive_member_never_creates_destination() {
        let mut header = tar::Header::new_gnu();
        header.set_size(5);
        header.set_mode(0o600);
        header.set_entry_type(tar::EntryType::Regular);
        header.as_mut_bytes()[..10].copy_from_slice(b"../outside");
        header.set_cksum();
        let mut builder = tar::Builder::new(Vec::new());
        builder.append(&header, &b"hello"[..]).unwrap();
        let compressed = zstd::encode_all(&builder.into_inner().unwrap()[..], 3).unwrap();
        let dest = temp();
        assert!(recover_payload(&Config::default(), "a.tar.zst", &compressed, &dest).is_err());
        assert!(!dest.exists());
    }
    #[test]
    fn sql_export_is_exact_and_never_overwrites() {
        let dest = temp();
        let compressed = zstd::encode_all(&b"SELECT 1;"[..], 3).unwrap();
        let report = recover_payload(&Config::default(), "a.sql.zst", &compressed, &dest).unwrap();
        assert_eq!(report.0, "sql_export");
        assert_eq!(std::fs::read(&dest).unwrap(), b"SELECT 1;");
        assert!(recover_payload(&Config::default(), "a.sql.zst", &compressed, &dest).is_err());
        std::fs::remove_file(dest).unwrap();
    }
}
