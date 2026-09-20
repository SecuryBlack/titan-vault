use crate::config::Config;
use crate::engine::pipeline::BackupPipeline;
use crate::engine::retention::RetentionManager;
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tracing::info;

pub struct AutonomousScheduler {
    config: Config,
    pipeline: Arc<BackupPipeline>,
}

impl AutonomousScheduler {
    pub fn new(config: Config, pipeline: Arc<BackupPipeline>) -> Self {
        Self { config, pipeline }
    }

    pub async fn run(&self, mut shutdown_rx: watch::Receiver<bool>) -> Result<()> {
        if !self.config.schedule.enabled {
            info!("Scheduler is disabled in configuration. Running in manual/on-demand mode only.");
            while !*shutdown_rx.borrow() {
                if shutdown_rx.changed().await.is_err() {
                    break;
                }
            }
            return Ok(());
        }

        info!("Starting autonomous backup scheduler loop...");

        // Comprobación cada 60 segundos
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        let mut last_hourly_hour: i32 = -1;
        let mut last_daily_day: u32 = 0;

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let now = chrono::Local::now();
                    let current_hour = now.format("%H").to_string().parse::<i32>().unwrap_or(-1);
                    let current_minute = now.format("%M").to_string().parse::<i32>().unwrap_or(-1);
                    let current_day = now.format("%d").to_string().parse::<u32>().unwrap_or(0);

                    // 1. Hourly check (al minuto 0 de cada hora)
                    if current_minute == 0 && current_hour != last_hourly_hour {
                        last_hourly_hour = current_hour;
                        info!("Triggering scheduled HOURLY backup...");
                        let p = self.pipeline.clone();
                        tokio::spawn(async move {
                            p.run_all("hourly").await;
                        });
                    }

                    // 2. Daily check (a las 02:00 de la madrugada)
                    if current_hour == 2 && current_minute == 0 && current_day != last_daily_day {
                        last_daily_day = current_day;
                        info!("Triggering scheduled DAILY backup and GFS retention prune...");
                        let p = self.pipeline.clone();
                        let retention = self.config.retention.clone();
                        tokio::spawn(async move {
                            p.run_all("daily").await;
                            for target in p.storage().get_targets() {
                                let _ = RetentionManager::prune_target(&p.storage(), &target.id, &retention).await;
                            }
                        });
                    }
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("Autonomous scheduler received shutdown signal.");
                        break;
                    }
                }
            }
        }

        Ok(())
    }
}
