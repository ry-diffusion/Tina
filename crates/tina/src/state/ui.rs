use std::sync::Arc;

use chrono::Datelike;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel, Weak};
use tina_worker::{Account, ChatRow, MessageRow, TinaWorker};

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

pub(crate) fn setup_settings_callbacks(
    handle: &Weak<Tina>,
    tx: tokio::sync::mpsc::UnboundedSender<super::messages::UIMessage>,
) {
    handle
        .clone()
        .upgrade_in_event_loop(move |ui| {
            let settings = ui.global::<crate::AppSettings>();
            settings.on_logout(|| {
                tracing::info!("Logout requested");
            });
            let tx_repair = tx.clone();
            settings.on_repair(move || {
                let _ = tx_repair.send(super::messages::UIMessage::RepairRequested);
            });
        })
        .ok();
}

/// Liga/desliga o estado `AppSettings.repairing` (controla o overlay).
pub(crate) fn set_repairing(handle: &Weak<Tina>, repairing: bool) {
    handle
        .clone()
        .upgrade_in_event_loop(move |ui| {
            ui.global::<crate::AppSettings>().set_repairing(repairing);
        })
        .ok();
}

/// Atualiza a barra de progresso da reconciliação.
pub(crate) fn update_repair_progress(
    handle: &Weak<Tina>,
    stage: &str,
    current: i64,
    total: i64,
    indeterminate: bool,
) {
    let stage = SharedString::from(stage);
    handle
        .clone()
        .upgrade_in_event_loop(move |ui| {
            let p = ui.global::<crate::RepairProgress>();
            p.set_stage(stage);
            p.set_current(current as i32);
            p.set_total(total as i32);
            p.set_indeterminate(indeterminate);
        })
        .ok();
}

pub(crate) fn setup_chat_callbacks(
    handle: &Weak<Tina>,
    tx: tokio::sync::mpsc::UnboundedSender<super::messages::UIMessage>,
) {
    handle
        .clone()
        .upgrade_in_event_loop(move |ui| {
            let chat_mgmt = ui.global::<crate::ChatManagement>();
            // Garante um VecModel mutável para upserts incrementais.
            chat_mgmt.set_chats(ModelRc::new(VecModel::<crate::ChatItem>::default()));

            // Mesma coisa para a janela do chat.
            let chat_view = ui.global::<crate::ChatViewModel>();
            chat_view.set_messages(ModelRc::new(VecModel::<crate::MessageItem>::default()));

            let tx_load = tx.clone();
            chat_mgmt.on_load_chats(move || {
                let _ = tx_load.send(super::messages::UIMessage::LoadChats);
            });

            let tx_select = tx.clone();
            chat_mgmt.on_select_chat(move |chat_id| {
                let id = chat_id.to_string();
                let opt = if id.is_empty() { None } else { Some(id) };
                let _ = tx_select.send(super::messages::UIMessage::SetActiveChat(opt));
            });
        })
        .ok();
}

/// Aplica um lote de `ChatRow` à lista exibida na UI: faz upsert por
/// `chat_id` e re-ordena por `last_message_ts` (desc). Mantém um único
/// modelo, sem reconstruir do zero — atualizações in-place não fazem a
/// `ListView` do Slint piscar.
pub(crate) fn apply_chats_upserted(handle: &Weak<Tina>, rows: Vec<ChatRow>) {
    handle
        .clone()
        .upgrade_in_event_loop(move |ui| {
            let chat_mgmt = ui.global::<crate::ChatManagement>();
            let model_rc = chat_mgmt.get_chats();

            // Indexa o modelo atual por chat_id.
            let n = model_rc.row_count();
            let mut existing: std::collections::HashMap<SharedString, usize> =
                std::collections::HashMap::with_capacity(n);
            for i in 0..n {
                if let Some(item) = model_rc.row_data(i) {
                    existing.insert(item.id.clone(), i);
                }
            }

            // Aplica updates ou empilha pra inserir.
            let mut to_insert: Vec<crate::ChatItem> = Vec::new();
            for row in rows {
                let item = chat_row_to_item(&row);
                if let Some(&idx) = existing.get(&item.id) {
                    model_rc.set_row_data(idx, item);
                } else {
                    to_insert.push(item);
                }
            }

            // Para inserir, precisamos do VecModel concreto. Tentamos castar.
            if !to_insert.is_empty() {
                if let Some(vm) = model_rc.as_any().downcast_ref::<VecModel<crate::ChatItem>>() {
                    for item in to_insert {
                        vm.push(item);
                    }
                }
            }

            // Re-ordena por timestamp desc, fixados no topo os pinned.
            sort_chat_model(&model_rc);
        })
        .ok();
}

fn chat_row_to_item(row: &ChatRow) -> crate::ChatItem {
    let preview = row
        .last_message_preview
        .clone()
        .unwrap_or_default();
    let preview = if row.last_message_from_me && !preview.is_empty() {
        format!("Você: {}", preview)
    } else {
        preview
    };
    let last_ts = row.last_message_ts.unwrap_or(0);
    let timestamp = if last_ts > 0 {
        format_timestamp(last_ts)
    } else {
        String::new()
    };

    crate::ChatItem {
        id: SharedString::from(row.chat_id.clone()),
        kind: SharedString::from(row.kind.clone()),
        name: SharedString::from(row.name.clone()),
        last_message: SharedString::from(preview),
        timestamp: SharedString::from(timestamp),
        last_ts: last_ts as i32,
        unread_count: row.unread_count as i32,
        pinned: row.pinned,
        avatar: Default::default(),
    }
}

fn sort_chat_model(model_rc: &ModelRc<crate::ChatItem>) {
    let Some(vm) = model_rc.as_any().downcast_ref::<VecModel<crate::ChatItem>>() else {
        return;
    };
    let n = vm.row_count();
    if n <= 1 {
        return;
    }
    let mut all: Vec<crate::ChatItem> = (0..n).filter_map(|i| vm.row_data(i)).collect();
    all.sort_by(|a, b| {
        // Pinned no topo, depois last_ts desc.
        b.pinned
            .cmp(&a.pinned)
            .then(b.last_ts.cmp(&a.last_ts))
            .then_with(|| a.name.as_str().cmp(b.name.as_str()))
    });
    for (i, item) in all.into_iter().enumerate() {
        vm.set_row_data(i, item);
    }
}

pub(crate) fn format_timestamp(timestamp: i64) -> String {
    use chrono::{DateTime, Local, Utc};
    let dt = DateTime::<Utc>::from_timestamp(timestamp, 0).unwrap_or_else(Utc::now);
    let local: DateTime<Local> = dt.into();
    let now = Local::now();
    if local.date_naive() == now.date_naive() {
        local.format("%H:%M").to_string()
    } else if local.year() == now.year() {
        local.format("%d/%m").to_string()
    } else {
        local.format("%d/%m/%y").to_string()
    }
}

#[allow(dead_code)]
pub(crate) async fn load_account_data(
    _worker: &Arc<TinaWorker>,
    _account_id: &str,
) -> color_eyre::Result<()> {
    Ok(())
}

// =====================================================================
// Chat view (right pane)
// =====================================================================

/// Atualiza os metadados do chat ativo (nome/kind/id) e troca o modelo de
/// mensagens por uma carga inicial. Passar `chat_id = None` limpa a janela.
pub(crate) fn apply_chat_opened(
    handle: &Weak<Tina>,
    chat_id: Option<&str>,
    chat_name: Option<&str>,
    chat_kind: Option<&str>,
    initial_messages: Vec<MessageRow>,
) {
    let id = SharedString::from(chat_id.unwrap_or(""));
    let name = SharedString::from(chat_name.unwrap_or(""));
    let kind = SharedString::from(chat_kind.unwrap_or(""));

    handle
        .clone()
        .upgrade_in_event_loop(move |ui| {
            // Define qual chat está ativo (para a UI flipar pra ChatContentView).
            let chat_mgmt = ui.global::<crate::ChatManagement>();
            chat_mgmt.set_active_chat_id(id.clone());

            let chat_view = ui.global::<crate::ChatViewModel>();
            chat_view.set_chat_id(id);
            chat_view.set_chat_name(name);
            chat_view.set_chat_kind(kind);

            let items: Vec<crate::MessageItem> =
                initial_messages.iter().map(message_row_to_item).collect();
            chat_view.set_messages(ModelRc::new(VecModel::from(items)));
        })
        .ok();
}

/// Acrescenta mensagens novas ao chat ativo, preservando a posição de
/// scroll (apenas push).
pub(crate) fn apply_messages_appended(
    handle: &Weak<Tina>,
    chat_id: &str,
    messages: Vec<MessageRow>,
) {
    let chat_id = SharedString::from(chat_id);
    handle
        .clone()
        .upgrade_in_event_loop(move |ui| {
            let chat_view = ui.global::<crate::ChatViewModel>();
            // Só aplica se o chat ainda é o ativo.
            if chat_view.get_chat_id() != chat_id {
                return;
            }
            let model_rc = chat_view.get_messages();
            let Some(vm) = model_rc
                .as_any()
                .downcast_ref::<VecModel<crate::MessageItem>>()
            else {
                return;
            };
            // Indexa por message_id para evitar duplicatas (whatsmeow pode
            // re-entregar a mesma mensagem em history sync + push).
            let mut existing: std::collections::HashSet<SharedString> =
                std::collections::HashSet::<SharedString>::with_capacity(vm.row_count());
            for i in 0..vm.row_count() {
                if let Some(item) = vm.row_data(i) {
                    existing.insert(item.id);
                }
            }
            for msg in &messages {
                let item = message_row_to_item(msg);
                if !existing.contains(&item.id) {
                    vm.push(item);
                }
            }
        })
        .ok();
}

fn message_row_to_item(row: &MessageRow) -> crate::MessageItem {
    let content = row.content.clone().unwrap_or_default();
    let timestamp = if row.timestamp > 0 {
        format_message_time(row.timestamp)
    } else {
        String::new()
    };
    let sender_name = row.sender_name.clone().unwrap_or_default();
    crate::MessageItem {
        id: SharedString::from(row.message_id.clone()),
        from_me: row.is_from_me,
        sender_name: SharedString::from(sender_name),
        content: SharedString::from(content),
        timestamp: SharedString::from(timestamp),
        message_type: SharedString::from(row.message_type.clone()),
    }
}

fn format_message_time(timestamp: i64) -> String {
    use chrono::{DateTime, Local, Utc};
    let dt = DateTime::<Utc>::from_timestamp(timestamp, 0).unwrap_or_else(Utc::now);
    let local: DateTime<Local> = dt.into();
    let now = Local::now();
    if local.date_naive() == now.date_naive() {
        local.format("%H:%M").to_string()
    } else if local.year() == now.year() {
        local.format("%d/%m %H:%M").to_string()
    } else {
        local.format("%d/%m/%y %H:%M").to_string()
    }
}
