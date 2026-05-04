// Root model: scene state + child controllers + the toast helper.

use relm4::Controller;

use crate::components::login::LoginPage;
use crate::components::main_page::MainPage;
use crate::components::settings::Settings;
use crate::service::ServiceWorker;

use super::messages::{ConnectionStatus, Scene};

pub struct AppModel {
    pub(super) scene: Scene,
    /// Scene to restore when `Scene::Repairing` ends. Set on the
    /// transition into `Repairing`; cleared on `RepairEnded`.
    pub(super) pre_repair_scene: Option<Scene>,
    pub(super) error: Option<String>,
    pub(super) repairing: bool,
    pub(super) repair_stage: String,
    pub(super) repair_current: i64,
    pub(super) repair_total: i64,
    pub(super) repair_indeterminate: bool,
    /// 0..100 — last value reported by `WorkerEvent::HistorySyncProgress`.
    pub(super) sync_progress: u32,
    /// Last `HistorySync.SyncType` enum string ("INITIAL_BOOTSTRAP",
    /// "RECENT", …) — humanised by the syncing page for the description.
    pub(super) sync_type: String,
    /// Worker-reported link state. `Connecting` until the first
    /// `Connected` event lands.
    pub(super) connection: ConnectionStatus,
    pub(super) phone: Option<String>,
    pub(super) service: ServiceWorker,
    pub(super) login: Controller<LoginPage>,
    pub(super) main: Controller<MainPage>,
    /// Held across the app's lifetime — the dialog is presented on
    /// demand from the profile menu and dismissed by the user. We
    /// keep its widget around so the next open is instant.
    pub(super) settings: Controller<Settings>,
    pub(super) toast_overlay: adw::ToastOverlay,
}

impl AppModel {
    /// Human label for the syncing page.
    pub(super) fn sync_stage_label(&self) -> String {
        match self.sync_type.as_str() {
            "" => "Pulling your message history…".to_string(),
            "INITIAL_BOOTSTRAP" => "Pulling your message history…".to_string(),
            "INITIAL_STATUS_V3" => "Syncing status updates…".to_string(),
            "RECENT" => "Pulling recent messages…".to_string(),
            "FULL" => "Pulling your full history…".to_string(),
            "PUSH_NAME" => "Syncing contacts…".to_string(),
            "NON_BLOCKING_DATA" => "Syncing extras…".to_string(),
            "ON_DEMAND" => "Pulling requested history…".to_string(),
            other => format!("Syncing ({other})…"),
        }
    }

    pub(super) fn sync_fraction(&self) -> f64 {
        (self.sync_progress as f64 / 100.0).clamp(0.0, 1.0)
    }

    pub(super) fn sync_percent_text(&self) -> String {
        format!("{}%", self.sync_progress.min(100))
    }

    /// Repair page heading. Falls back to "Starting…" before the first
    /// `RepairProgress` event arrives so the page never looks empty.
    pub(super) fn repair_title(&self) -> &str {
        if self.repair_stage.is_empty() {
            "Starting…"
        } else {
            "Repairing…"
        }
    }

    pub(super) fn repair_description(&self) -> String {
        if self.repair_stage.is_empty() {
            "Reading your data from WhatsApp.".to_string()
        } else {
            self.repair_stage.clone()
        }
    }

    pub(super) fn repair_fraction(&self) -> f64 {
        if self.repair_total > 0 && !self.repair_indeterminate {
            (self.repair_current as f64 / self.repair_total as f64).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    pub(super) fn repair_progress_text(&self) -> String {
        if self.repair_total > 0 && !self.repair_indeterminate {
            format!("{} / {}", self.repair_current, self.repair_total)
        } else {
            String::new()
        }
    }
}

impl AppModel {
    pub(super) fn toast(&self, text: String) {
        let toast = adw::Toast::builder().title(&text).timeout(3).build();
        self.toast_overlay.add_toast(toast);
    }
}
