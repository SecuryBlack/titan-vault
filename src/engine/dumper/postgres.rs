use crate::config::DatabaseSource;
use anyhow::{anyhow, Context, Result};
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tracing::info;

pub struct PostgresDumper;

impl PostgresDumper {
    pub async fn dump(source: &DatabaseSource) -> Result<Vec<u8>> {
        info!("Starting PostgreSQL dump for database '{}'...", source.database);

        let mut cmd = if let Some(container) = &source.container_name {
            let mut c = Command::new("docker");
            c.arg("exec")
                .arg("-i")
                .arg(container)
                .arg("pg_dump")
                .arg("--no-password");

            if let Some(user) = &source.user {
                c.arg("-U").arg(user);
            }
            c.arg("-d").arg(&source.database);
            c
        } else {
            let mut c = Command::new("pg_dump");
            c.arg("--no-password");

            if let Some(host) = &source.host {
                c.arg("-h").arg(host);
            }
            if let Some(port) = source.port {
                c.arg("-p").arg(port.to_string());
            }
            if let Some(user) = &source.user {
                c.arg("-U").arg(user);
            }
            if let Some(password) = &source.password {
                c.env("PGPASSWORD", password);
            }
            c.arg("-d").arg(&source.database);
            c
        };

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .context("failed to spawn pg_dump command (check that docker or pg_dump is installed)")?;

        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("failed to capture pg_dump stdout"))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("failed to capture pg_dump stderr"))?;

        let mut dump_data = Vec::new();
        let mut err_data = Vec::new();

        tokio::try_join!(
            stdout.read_to_end(&mut dump_data),
            stderr.read_to_end(&mut err_data)
        )?;

        let status = child.wait().await?;
        if !status.success() {
            let err_msg = String::from_utf8_lossy(&err_data);
            return Err(anyhow!("pg_dump exited with error ({}): {}", status, err_msg.trim()));
        }

        info!(
            "PostgreSQL dump completed for '{}' ({} bytes)",
            source.database,
            dump_data.len()
        );
        Ok(dump_data)
    }
}
