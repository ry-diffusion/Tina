// Root model: scene state + child controllers + the toast helper.

use relm4::Controller;

use crate::components::login::LoginPage;
use crate::components::main_page::MainPage;
use crate::service::ServiceWorker;

use super::messages::Scene;

pub struct AppModel {
    pub(super) scene: Scene,
    pub(super) error: Option<String>,
    pub(super) repairing: bool,
    pub(super) repair_stage: String,
    pub(super) repair_current: i64,
    pub(super) repair_total: i64,
    pub(super) repair_indeterminate: bool,
    pub(super) phone: Option<String>,
    pub(super) service: ServiceWorker,
    pub(super) login: Controller<LoginPage>,
    pub(super) main: Controller<MainPage>,
    pub(super) toast_overlay: adw::ToastOverlay,
}

impl AppModel {
    pub(super) fn toast(&self, text: String) {
        let toast = adw::Toast::builder().title(&text).timeout(3).build();
        self.toast_overlay.add_toast(toast);
    }
}
