use crate::config::FilesystemSource;
use anyhow::{Context, Result};
use std::path::Path;
use tar::Builder;
use tracing::info;

pub struct FilesDumper;

impl FilesDumper {
    pub async fn dump(source: &FilesystemSource) -> Result<Vec<u8>> {
        let source_clone = source.clone();
        tokio::task::spawn_blocking(move || Self::dump_sync(&source_clone)).await?
    }

    fn dump_sync(source: &FilesystemSource) -> Result<Vec<u8>> {
        info!("Starting filesystem archive for '{}'...", source.name);
        let mut builder = Builder::new(Vec::new());

        for path in &source.paths {
            if !path.exists() {
                continue;
            }
            if path.is_file() {
                let name = path.file_name().unwrap_or_default();
                builder
                    .append_path_with_name(path, name)
                    .context(format!("failed to append file {:?}", path))?;
            } else if path.is_dir() {
                let dir_name = path.file_name().unwrap_or_default();
                Self::append_dir_recursive(&mut builder, path, Path::new(dir_name), &source.excludes)?;
            }
        }

        let tar_data = builder.into_inner()?;
        info!(
            "Filesystem archive completed for '{}' ({} bytes uncompressed)",
            source.name,
            tar_data.len()
        );
        Ok(tar_data)
    }

    fn append_dir_recursive(
        builder: &mut Builder<Vec<u8>>,
        real_path: &Path,
        arch_path: &Path,
        excludes: &[String],
    ) -> Result<()> {
        let entries = std::fs::read_dir(real_path)?;

        for entry in entries {
            let entry = entry?;
            let entry_path = entry.path();
            let entry_name = entry.file_name();
            let sub_arch_path = arch_path.join(entry_name);

            // Check exclusion patterns
            let path_str = entry_path.to_string_lossy();
            let should_exclude = excludes.iter().any(|pattern| {
                let pat = pattern.trim_matches('*');
                path_str.contains(pat)
            });

            if should_exclude {
                continue;
            }

            if entry_path.is_dir() {
                Self::append_dir_recursive(builder, &entry_path, &sub_arch_path, excludes)?;
            } else if entry_path.is_file() {
                let _ = builder.append_path_with_name(&entry_path, &sub_arch_path);
            }
        }

        Ok(())
    }
}
