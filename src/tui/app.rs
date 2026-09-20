use crate::config::Config;
use crate::engine::pipeline::BackupPipeline;
use crate::storage::SnapshotMeta;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Sources,
    Targets,
    PolicyCrypto,
    Snapshots,
}

impl Tab {
    pub fn all() -> &'static [Tab] {
        &[Tab::Sources, Tab::Targets, Tab::PolicyCrypto, Tab::Snapshots]
    }

    pub fn title(&self) -> &'static str {
        match self {
            Tab::Sources => "1. Sources (DB & Files)",
            Tab::Targets => "2. Targets (R2/Hetzner/S3)",
            Tab::PolicyCrypto => "3. Policy & Crypto",
            Tab::Snapshots => "4. Snapshots & Restore",
        }
    }
}

pub struct App {
    pub config: Config,
    pub config_path: PathBuf,
    pub current_tab: Tab,
    pub selected_index: usize,
    pub status_message: String,
    pub status_is_error: bool,
    pub test_result: Option<String>,
    pub live_backup_in_progress: bool,
    pub live_backup_log: Vec<String>,
    pub snapshots: Vec<SnapshotMeta>,
    pub is_editing: bool,
    pub edit_buffer: String,
    pub should_quit: bool,
    pub pipeline: Option<Arc<BackupPipeline>>,
}

impl App {
    pub fn new(config: Config, config_path: PathBuf) -> Self {
        let pipeline = BackupPipeline::new(config.clone()).ok().map(Arc::new);
        Self {
            config,
            config_path,
            current_tab: Tab::Sources,
            selected_index: 0,
            status_message: "Press [Tab] to switch views, [S] to save config, [B] to run backup now".to_string(),
            status_is_error: false,
            test_result: None,
            live_backup_in_progress: false,
            live_backup_log: Vec::new(),
            snapshots: Vec::new(),
            is_editing: false,
            edit_buffer: String::new(),
            should_quit: false,
            pipeline,
        }
    }

    pub fn next_tab(&mut self) {
        let tabs = Tab::all();
        let current_pos = tabs.iter().position(|t| *t == self.current_tab).unwrap_or(0);
        let next_pos = (current_pos + 1) % tabs.len();
        self.current_tab = tabs[next_pos];
        self.selected_index = 0;
        self.test_result = None;
    }

    pub fn prev_tab(&mut self) {
        let tabs = Tab::all();
        let current_pos = tabs.iter().position(|t| *t == self.current_tab).unwrap_or(0);
        let prev_pos = if current_pos == 0 { tabs.len() - 1 } else { current_pos - 1 };
        self.current_tab = tabs[prev_pos];
        self.selected_index = 0;
        self.test_result = None;
    }

    pub fn move_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn move_down(&mut self, max_items: usize) {
        if max_items > 0 && self.selected_index + 1 < max_items {
            self.selected_index += 1;
        }
    }

    pub fn save_config(&mut self) {
        match self.config.save_to(&self.config_path) {
            Ok(_) => {
                self.status_message = format!("Config saved successfully to {}", self.config_path.display());
                self.status_is_error = false;
                // Reload pipeline with updated config
                self.pipeline = BackupPipeline::new(self.config.clone()).ok().map(Arc::new);
            }
            Err(e) => {
                self.status_message = format!("Failed to save config: {e}");
                self.status_is_error = true;
            }
        }
    }
}
