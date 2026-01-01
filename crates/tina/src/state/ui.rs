use std::sync::Arc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel, Weak};
use tina_worker::{Account, TinaWorker};

use crate::{Scene, Tina};

use super::qr::render_qr_image;

pub(crate) fn show_scene(handle: &Weak<Tina>, scene: Scene) {
    handle
        .clone()
        .upgrade_in_event_loop(move |ui| {
            ui.set_current_scene(scene);
        })
        .ok();
}

pub(crate) fn update_qr_code(handle: &Weak<Tina>, qr: &str) {
    let qr_data = qr.to_owned();

    handle
        .clone()
        .upgrade_in_event_loop(move |ui| {
            let am = ui.global::<crate::AccountManagement>();
            let image = render_qr_image(&qr_data).unwrap_or_default();
            am.set_qr_code(image);
        })
        .ok();
}

pub(crate) fn update_account_list(handle: &Weak<Tina>, accounts: &[Account]) {
    let account_strings: Vec<SharedString> = accounts
        .iter()
        .map(|a| SharedString::from(a.id.clone()))
        .collect();

    handle
        .clone()
        .upgrade_in_event_loop(move |ui| {
            let am = ui.global::<crate::AccountManagement>();
            let model = VecModel::from(account_strings);
            am.set_accounts(ModelRc::new(model));
        })
        .ok();
}

pub(crate) fn set_selected_account(handle: &Weak<Tina>, account_id: Option<&str>) {
    let selected = SharedString::from(account_id.unwrap_or_default());

    handle
        .clone()
        .upgrade_in_event_loop(move |ui| {
            let am = ui.global::<crate::AccountManagement>();
            am.set_selected_account(selected.clone());
        })
        .ok();
}

pub(crate) fn show_error(handle: &Weak<Tina>, _msg: &str) {
    handle
        .clone()
        .upgrade_in_event_loop(move |ui| {
            let _fm = ui.global::<crate::FailureManagment>();
            ui.set_current_scene(Scene::FatalError);
        })
        .ok();
}

#[tracing::instrument(skip(handle))]
pub(crate) fn crash_app(handle: &Weak<Tina>, msg: &str) {
    let shared_errmsg = SharedString::from(String::from(msg));

    handle
        .clone()
        .upgrade_in_event_loop(move |ui| {
            let fm = ui.global::<crate::FailureManagment>();
            fm.set_error(shared_errmsg);
            ui.set_current_scene(Scene::FatalError);
        })
        .expect("Failed to crash the app");
}

#[allow(dead_code)]
pub(crate) async fn load_account_data(
    worker: &Arc<TinaWorker>,
    account_id: &str,
) -> color_eyre::Result<()> {
    let _contacts = worker.get_contacts(account_id).await?;
    let _chats = worker.get_chats(account_id).await?;
    Ok(())
}
