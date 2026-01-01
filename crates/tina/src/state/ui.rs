use std::sync::Arc;

use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel, Weak};
use tina_worker::{Account, TinaWorker};

use crate::{Scene, Tina};
use crate::jid_utils::format_jid_for_display;

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

/// Update user profile information in the UI
pub(crate) fn update_user_profile(
    handle: &Weak<Tina>,
    name: Option<&str>,
    phone_number: Option<&str>,
    status: Option<&str>,
) {
    let name = SharedString::from(name.unwrap_or("User"));
    let phone = SharedString::from(phone_number.unwrap_or(""));
    let status = SharedString::from(status.unwrap_or("Hey there! I am using Tina."));

    handle
        .clone()
        .upgrade_in_event_loop(move |ui| {
            let profile = ui.global::<crate::UserProfile>();
            profile.set_name(name);
            profile.set_phone_number(phone);
            profile.set_status(status);
        })
        .ok();
}

/// Setup callbacks for app settings
pub(crate) fn setup_settings_callbacks(handle: &Weak<Tina>) {
    handle
        .clone()
        .upgrade_in_event_loop(move |ui| {
            let settings = ui.global::<crate::AppSettings>();

            settings.on_logout(|| {
                tracing::info!("Logout requested");
                // TODO: Implement logout logic
            });
        })
        .ok();
}

/// Setup callbacks for chat management
pub(crate) fn setup_chat_callbacks(
    handle: &Weak<Tina>,
    tx: tokio::sync::mpsc::UnboundedSender<super::messages::UIMessage>,
) {
    handle
        .clone()
        .upgrade_in_event_loop(move |ui| {
            let chat_mgmt = ui.global::<crate::ChatManagement>();
            let tx_clone = tx.clone();

            chat_mgmt.on_load_chats(move || {
                let _ = tx_clone.send(super::messages::UIMessage::LoadChats);
            });
        })
        .ok();
}

/// Update chats list in the UI
pub(crate) fn update_chats_list(handle: &Weak<Tina>, chats: &[String]) {
    let chat_jids: Vec<String> = chats.iter().map(|s| s.clone()).collect();

    handle
        .clone()
        .upgrade_in_event_loop(move |ui| {
            let chat_items: Vec<crate::ChatItem> = chat_jids
                .iter()
                .map(|jid| crate::ChatItem {
                    id: SharedString::from(jid.clone()),
                    name: format_jid_for_display(jid),
                    last_message: SharedString::from(""),
                    timestamp: SharedString::from(""),
                    unread_count: 0,
                    avatar: Default::default(),
                })
                .collect();

            let chat_mgmt = ui.global::<crate::ChatManagement>();
            let model = VecModel::from(chat_items);
            chat_mgmt.set_chats(ModelRc::new(model));
        })
        .ok();
}

/// Update a specific chat preview
pub(crate) fn update_chat_preview(
    handle: &Weak<Tina>,
    chat_jid: &str,
    last_message: &str,
    timestamp: &str,
) {
    let chat_jid = SharedString::from(chat_jid);
    let last_message = SharedString::from(last_message);
    let timestamp = SharedString::from(timestamp);

    handle
        .clone()
        .upgrade_in_event_loop(move |ui| {
            let chat_mgmt = ui.global::<crate::ChatManagement>();
            let chats_model = chat_mgmt.get_chats();

            // Find and update the chat
            for i in 0..chats_model.row_count() {
                if let Some(chat) = chats_model.row_data(i) {
                    if chat.id == chat_jid {
                        let updated_chat = crate::ChatItem {
                            id: chat.id,
                            name: chat.name,
                            last_message: last_message.clone(),
                            timestamp: timestamp.clone(),
                            unread_count: chat.unread_count,
                            avatar: chat.avatar,
                        };
                        chats_model.set_row_data(i, updated_chat);
                        break;
                    }
                }
            }
        })
        .ok();
}

/// Update a specific chat name
pub(crate) fn update_chat_name(handle: &Weak<Tina>, chat_jid: &str, name: &str) {
    let chat_jid = SharedString::from(chat_jid);
    let name = SharedString::from(name);

    handle
        .clone()
        .upgrade_in_event_loop(move |ui| {
            let chat_mgmt = ui.global::<crate::ChatManagement>();
            let chats_model = chat_mgmt.get_chats();

            // Find and update the chat
            for i in 0..chats_model.row_count() {
                if let Some(chat) = chats_model.row_data(i) {
                    if chat.id == chat_jid {
                        let updated_chat = crate::ChatItem {
                            id: chat.id,
                            name: name.clone(),
                            last_message: chat.last_message,
                            timestamp: chat.timestamp,
                            unread_count: chat.unread_count,
                            avatar: chat.avatar,
                        };
                        chats_model.set_row_data(i, updated_chat);
                        break;
                    }
                }
            }
        })
        .ok();
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
