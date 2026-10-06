use crate::engine::retention::RetentionManager;
use crate::state::SharedVaultState;
use anyhow::{bail, Result};
use chrono::{Datelike, Timelike};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tracing::{error, info, warn};

/// Standard numeric five-field cron, evaluated in the host's local timezone.
/// Lists, inclusive ranges and positive steps are supported; Sunday is 0 or 7.
#[derive(Debug)]
pub(crate) struct CronExpression {
    fields: Vec<Vec<u32>>,
    day_any: bool,
    weekday_any: bool,
}

impl CronExpression {
    pub(crate) fn parse(expression: &str) -> Result<Self> {
        let parts: Vec<_> = expression.split_whitespace().collect();
        if parts.len() != 5 {
            bail!("Expected five numeric cron fields");
        }
        let mut fields = Vec::new();
        for (part, (min, max)) in parts
            .iter()
            .zip([(0, 59), (0, 23), (1, 31), (1, 12), (0, 7)])
        {
            let mut values = Vec::new();
            for item in part.split(',') {
                let mut step_parts = item.split('/');
                let base = step_parts.next().unwrap_or("");
                let step = step_parts
                    .next()
                    .map(str::parse::<u32>)
                    .transpose()?
                    .unwrap_or(1);
                if step == 0 || step_parts.next().is_some() {
                    bail!("Invalid cron step");
                }
                let (start, end) = if base == "*" {
                    (min, max)
                } else if let Some((a, b)) = base.split_once('-') {
                    (a.parse::<u32>()?, b.parse::<u32>()?)
                } else {
                    let value = base.parse::<u32>()?;
                    (value, if item.contains('/') { max } else { value })
                };
                if start < min || end > max || start > end {
                    bail!("Cron field outside {min}..={max}");
                }
                values.extend((start..=end).step_by(step as usize));
            }
            fields.push(values);
        }
        Ok(Self {
            fields,
            day_any: parts[2].starts_with('*'),
            weekday_any: parts[4].starts_with('*'),
        })
    }

    fn matches<T: chrono::TimeZone>(&self, now: &chrono::DateTime<T>) -> bool {
        let weekday = now.weekday().num_days_from_sunday();
        let day_match = self.fields[2].contains(&now.day());
        let weekday_match = self.fields[4].iter().any(|&d| d % 7 == weekday);
        let calendar_match = if self.day_any || self.weekday_any {
            day_match && weekday_match
        } else {
            day_match || weekday_match
        };
        self.fields[0].contains(&now.minute())
            && self.fields[1].contains(&now.hour())
            && self.fields[3].contains(&now.month())
            && calendar_match
    }
}

pub struct AutonomousScheduler {
    state: Arc<SharedVaultState>,
}

impl AutonomousScheduler {
    pub fn new(state: Arc<SharedVaultState>) -> Self {
        Self { state }
    }

    pub async fn run(&self, mut shutdown_rx: watch::Receiver<bool>) -> Result<()> {
        info!("Starting five-level backup scheduler (host local timezone)");
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last_run: HashMap<String, i64> = HashMap::new();
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if *shutdown_rx.borrow() { break; }
                    let (schedule, retention) = {
                        let cfg = self.state.config.read().await;
                        (cfg.schedule.clone(), cfg.retention.clone())
                    };
                    if !schedule.enabled { continue; }
                    let now = chrono::Local::now();
                    let minute = now.timestamp().div_euclid(60);
                    // Capture all matching levels before awaiting any backup. Serialize jobs;
                    // a long hourly job must not make another matching level disappear.
                    let mut due = Vec::new();
                    for (level, expression) in schedule.levels() {
                        match CronExpression::parse(expression) {
                            Ok(cron) if cron.matches(&now) && last_run.get(level) != Some(&minute) => {
                                last_run.insert(level.to_string(), minute);
                                due.push(level.to_string());
                            }
                            Ok(_) => {},
                            Err(e) => error!("Invalid {level} schedule: {e}"),
                        }
                    }
                    for level in due {
                        if *shutdown_rx.borrow() { break; }
                        info!("Triggering scheduled {level} backup");
                        let pipeline = self.state.pipeline.read().await.clone();
                        let reports = pipeline.run_all(&level).await;
                        if reports.is_empty() || reports.iter().any(|r| !r.success) {
                            warn!("Skipping retention after failed/empty {level} backup");
                            continue;
                        }
                        for target in pipeline.storage().get_targets() {
                            if let Err(e) = RetentionManager::prune_target_level(&pipeline.storage(), &target.id, &retention, &level).await {
                                error!("Retention failed on {}: {e}", target.id);
                            }
                        }
                    }
                }
                changed = shutdown_rx.changed() => {
                    if changed.is_err() || *shutdown_rx.borrow() { break; }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    fn at(month: u32, day: u32, hour: u32, minute: u32) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc
            .with_ymd_and_hms(2026, month, day, hour, minute, 0)
            .unwrap()
    }
    #[test]
    fn defaults_match_all_five_infra_levels() {
        let schedule = crate::config::ScheduleConfig::default();
        let times = [
            at(1, 1, 7, 0),
            at(1, 1, 2, 0),
            at(1, 4, 3, 0),
            at(2, 1, 4, 0),
            at(1, 1, 5, 0),
        ];
        for ((_, expr), now) in schedule.levels().iter().zip(times) {
            assert!(CronExpression::parse(expr).unwrap().matches(&now));
            assert!(!CronExpression::parse(expr)
                .unwrap()
                .matches(&(now + chrono::Duration::minutes(1))));
        }
    }
    #[test]
    fn respects_changed_minutes_ranges_lists_and_steps() {
        let cron = CronExpression::parse("5,20-40/10 6-8 * */2 *").unwrap();
        assert!(cron.matches(&at(3, 2, 7, 30)));
        assert!(!cron.matches(&at(3, 2, 7, 31)));
        assert!(!cron.matches(&at(2, 2, 7, 30)));
        assert!(!cron.matches(&at(3, 2, 9, 30)));
    }
    #[test]
    fn posix_calendar_semantics_and_sunday_alias() {
        let cron = CronExpression::parse("0 3 1 * 7").unwrap();
        assert!(cron.matches(&at(1, 4, 3, 0))); // Sunday, not first
        assert!(cron.matches(&at(1, 1, 3, 0))); // First, not Sunday
        assert!(!cron.matches(&at(1, 2, 3, 0)));
        assert!(!CronExpression::parse("0 3 * * 7")
            .unwrap()
            .matches(&at(1, 1, 3, 0)));
    }
    #[test]
    fn rejects_invalid_cron() {
        for expr in [
            "",
            "0 2 * *",
            "60 2 * * *",
            "0 24 * * *",
            "0 2 0 * *",
            "0 2 * 13 *",
            "*/0 * * * *",
            "8-2 * * * *",
            "0 2 * * MON",
        ] {
            assert!(CronExpression::parse(expr).is_err(), "{expr}");
        }
    }
    #[test]
    fn old_configs_default_new_levels_and_heartbeats_fallback() {
        let cfg: crate::config::Config = toml::from_str("[schedule]\ncron = '15 2 * * *'\n[cloud]\nheartbeat_url = 'https://example.test/default'\n[cloud.heartbeat_urls]\nweekly = 'https://example.test/weekly'").unwrap();
        assert_eq!(cfg.schedule.weekly_cron, "0 3 * * 0");
        assert_eq!(
            cfg.cloud.heartbeat_for_level("weekly"),
            Some("https://example.test/weekly")
        );
        assert_eq!(
            cfg.cloud.heartbeat_for_level("daily"),
            Some("https://example.test/default")
        );
        cfg.schedule.validate().unwrap();
    }
}
