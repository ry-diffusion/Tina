// Root model: scene state + child controllers + the toast helper.

use relm4::Controller;
use crate::fl;

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
    /// True while we're showing the Syncing scene for a mid-session
    /// reconnect (as opposed to the initial bootstrap). Gates the Skip
    /// button and the live message counter.
    pub(super) reconnect_syncing: bool,
    /// Count of messages received via `MessagesAppended` since the
    /// current reconnect sync started. Shown in the Syncing scene
    /// description so the user can see progress without a % bar.
    pub(super) reconnect_messages_count: u32,
    /// Worker-reported link state. `Connecting` until the first
    /// `Connected` event lands.
    pub(super) connection: ConnectionStatus,
    pub(super) phone: Option<String>,
    pub(super) data_dir: std::path::PathBuf,
    pub(super) service: ServiceWorker,
    pub(super) login: Controller<LoginPage>,
    pub(super) main: Controller<MainPage>,
    /// Held across the app's lifetime — the dialog is presented on
    /// demand from the profile menu and dismissed by the user. We
    /// keep its widget around so the next open is instant.
    pub(super) settings: Controller<Settings>,
    /// Live download policy. Cloned out into MainInit so every chat
    /// tab reads the same value; the App keeps a clone here so it can
    /// update it from PreferencesLoaded / SetDownloadMethod.
    pub(super) media: crate::inventory::MediaInventory,
    pub(super) toast_overlay: adw::ToastOverlay,
}

impl AppModel {
    /// Human label for the syncing page.
    pub(super) fn sync_stage_label(&self) -> String {
        if self.reconnect_syncing {
            return fl!("sync-reconnect-description", "count" = self.reconnect_messages_count);
        }
        match self.sync_type.as_str() {
            "" | "INITIAL_BOOTSTRAP" => fl!("sync-stage-initial"),
            "INITIAL_STATUS_V3" => fl!("sync-stage-status-v3"),
            "RECENT" => fl!("sync-stage-recent"),
            "FULL" => fl!("sync-stage-full"),
            "PUSH_NAME" => fl!("sync-stage-push-name"),
            "NON_BLOCKING_DATA" => fl!("sync-stage-non-blocking"),
            "ON_DEMAND" => fl!("sync-stage-on-demand"),
            other => fl!("sync-stage-other", "type" = other),
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
    pub(super) fn repair_title(&self) -> String {
        if self.repair_stage.is_empty() {
            fl!("repair-title-starting")
        } else {
            fl!("repair-title-repairing")
        }
    }

    pub(super) fn repair_description(&self) -> String {
        if self.repair_stage.is_empty() {
            fl!("repair-description-starting")
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
