use std::rc::Rc;
use std::cell::RefCell;
use slint::{ComponentHandle, Model, ModelRc, VecModel, Weak};

use crate::state::{SharedAppState, AccountState, ChatState, MessageState};

slint::include_modules!();

fn format_timestamp(timestamp: i64) -> String {
    use std::time::{Duration, UNIX_EPOCH};
    
    let datetime = UNIX_EPOCH + Duration::from_secs(timestamp as u64);
    let now = std::time::SystemTime::now();
    
    if let Ok(duration) = now.duration_since(datetime) {
        let hours = duration.as_secs() / 3600;
        if hours < 24 {
            let secs = timestamp % 86400;
            let h = (secs / 3600) % 24;
            let m = (secs % 3600) / 60;
            return format!("{:02}:{:02}", h, m);
        } else if hours < 168 {
            let days = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
            let day_index = ((timestamp / 86400) + 4) % 7;
            return days[day_index as usize].to_string();
        }
    }
    
    let days_since_epoch = timestamp / 86400;
    let year = 1970 + (days_since_epoch / 365);
    format!("{}", year)
}

impl From<&AccountState> for AccountInfo {
    fn from(state: &AccountState) -> Self {
        Self {
            id: state.id.clone().into(),
            name: state.name.clone().into(),
            phone_number: state.phone_number.clone().unwrap_or_default().into(),
            is_connected: state.is_connected,
            is_syncing: state.is_syncing,
        }
    }
}

impl From<&ChatState> for ChatItem {
    fn from(state: &ChatState) -> Self {
        Self {
            jid: state.jid.clone().into(),
            name: state.name.clone().into(),
            last_message: state.last_message.clone().unwrap_or_default().into(),
            last_message_time: state.last_message_time
                .map(format_timestamp)
                .unwrap_or_default()
                .into(),
            unread_count: state.unread_count,
            is_group: state.is_group,
            is_selected: false,
        }
    }
}

impl From<&MessageState> for MessageItem {
    fn from(state: &MessageState) -> Self {
        Self {
            id: state.id.clone().into(),
            sender_name: state.sender_name.clone().into(),
            content: state.content.clone().into(),
            timestamp: format_timestamp(state.timestamp).into(),
            is_from_me: state.is_from_me,
            message_type: state.message_type.clone().into(),
        }
    }
}

thread_local! {
    static MESSAGES_MODEL: RefCell<Option<Rc<VecModel<MessageItem>>>> = const { RefCell::new(None) };
    static CHATS_MODEL: RefCell<Option<Rc<VecModel<ChatItem>>>> = const { RefCell::new(None) };
    static ACCOUNTS_MODEL: RefCell<Option<Rc<VecModel<AccountInfo>>>> = const { RefCell::new(None) };
}

#[derive(Clone)]
pub struct UiBridge {
    ui_handle: Weak<TinaApp>,
}

impl UiBridge {
    pub fn new(ui_handle: Weak<TinaApp>) -> Self {
        Self { ui_handle }
    }

    pub async fn sync_accounts(&self, state: &SharedAppState) {
        let accounts = {
            let state = state.read().await;
            state.accounts.iter().map(AccountInfo::from).collect::<Vec<_>>()
        };
        
        let handle = self.ui_handle.clone();
        slint::invoke_from_event_loop(move || {
            if let Some(ui) = handle.upgrade() {
                ACCOUNTS_MODEL.with(|cell| {
                    let mut model_ref = cell.borrow_mut();
                    if let Some(model) = model_ref.as_ref() {
                        update_vec_model_by_id(model, accounts, |item| item.id.to_string());
                    } else {
                        let model = Rc::new(VecModel::from(accounts));
                        ui.global::<AppState>().set_accounts(ModelRc::from(model.clone()));
                        *model_ref = Some(model);
                    }
                });
            }
        }).ok();
    }

    pub async fn sync_chats(&self, state: &SharedAppState) {
        let chats = {
            let state = state.read().await;
            let chats: Vec<ChatItem> = state.chats.iter().map(|c| {
                let mut item = ChatItem::from(c);
                item.is_selected = state.current_chat_jid.as_deref() == Some(&c.jid);
                item
            }).collect();
            chats
        };
        
        let handle = self.ui_handle.clone();
        slint::invoke_from_event_loop(move || {
            if let Some(ui) = handle.upgrade() {
                CHATS_MODEL.with(|cell| {
                    let mut model_ref = cell.borrow_mut();
                    if let Some(model) = model_ref.as_ref() {
                        update_vec_model_by_id(model, chats, |item| item.jid.to_string());
                    } else {
                        let model = Rc::new(VecModel::from(chats));
                        ui.global::<AppState>().set_chats(ModelRc::from(model.clone()));
                        *model_ref = Some(model);
                    }
                });
            }
        }).ok();
    }

    pub async fn sync_messages(&self, state: &SharedAppState) {
        let messages = {
            let state = state.read().await;
            state.messages.iter().map(MessageItem::from).collect::<Vec<_>>()
        };
        
        let handle = self.ui_handle.clone();
        slint::invoke_from_event_loop(move || {
            if let Some(ui) = handle.upgrade() {
                MESSAGES_MODEL.with(|cell| {
                    let mut model_ref = cell.borrow_mut();
                    if let Some(model) = model_ref.as_ref() {
                        update_vec_model_by_id(model, messages, |item| item.id.to_string());
                    } else {
                        let model = Rc::new(VecModel::from(messages));
                        ui.global::<AppState>().set_messages(ModelRc::from(model.clone()));
                        *model_ref = Some(model);
                    }
                });
            }
        }).ok();
    }

    pub async fn sync_current_account(&self, state: &SharedAppState) {
        let account_id = {
            let state = state.read().await;
            state.current_account_id.clone().unwrap_or_default()
        };
        
        let handle = self.ui_handle.clone();
        slint::invoke_from_event_loop(move || {
            if let Some(ui) = handle.upgrade() {
                ui.global::<AppState>().set_current_account_id(account_id.into());
            }
        }).ok();
    }

    pub async fn sync_current_chat(&self, state: &SharedAppState) {
        let (chat_jid, chat_name) = {
            let state = state.read().await;
            (
                state.current_chat_jid.clone().unwrap_or_default(),
                state.current_chat_name.clone().unwrap_or_default(),
            )
        };
        
        let handle = self.ui_handle.clone();
        slint::invoke_from_event_loop(move || {
            if let Some(ui) = handle.upgrade() {
                let app_state = ui.global::<AppState>();
                app_state.set_current_chat_jid(chat_jid.into());
                app_state.set_current_chat_name(chat_name.into());
            }
        }).ok();
    }

    pub async fn sync_loading(&self, state: &SharedAppState) {
        let is_loading = {
            let state = state.read().await;
            state.is_loading
        };
        
        let handle = self.ui_handle.clone();
        slint::invoke_from_event_loop(move || {
            if let Some(ui) = handle.upgrade() {
                ui.global::<AppState>().set_is_loading(is_loading);
            }
        }).ok();
    }

    pub async fn sync_status(&self, state: &SharedAppState) {
        let (status_message, sync_status) = {
            let state = state.read().await;
            (state.status_message.clone(), state.sync_status.clone())
        };
        
        let handle = self.ui_handle.clone();
        slint::invoke_from_event_loop(move || {
            if let Some(ui) = handle.upgrade() {
                let app_state = ui.global::<AppState>();
                app_state.set_status_message(status_message.into());
                app_state.set_sync_status(sync_status.into());
            }
        }).ok();
    }

    pub async fn sync_qr_dialog(&self, state: &SharedAppState) {
        let (show_qr, qr_data) = {
            let state = state.read().await;
            (state.show_qr_dialog, state.qr_code_data.clone().unwrap_or_default())
        };
        
        let handle = self.ui_handle.clone();
        slint::invoke_from_event_loop(move || {
            if let Some(ui) = handle.upgrade() {
                let app_state = ui.global::<AppState>();
                app_state.set_show_qr_dialog(show_qr);
                app_state.set_qr_code_data(qr_data.into());
            }
        }).ok();
    }
}

fn update_vec_model_by_id<T, F>(
    model: &VecModel<T>,
    new_items: Vec<T>,
    get_id: F,
) where
    T: Clone + 'static,
    F: Fn(&T) -> String,
{
    use std::collections::{HashMap, HashSet};
    
    let new_map: HashMap<String, T> = new_items
        .into_iter()
        .map(|item| (get_id(&item), item))
        .collect();
    let new_ids: HashSet<&String> = new_map.keys().collect();
    
    let mut i = 0;
    while i < model.row_count() {
        if let Some(item) = model.row_data(i) {
            let id = get_id(&item);
            if !new_ids.contains(&id) {
                model.remove(i);
                continue;
            }
        }
        i += 1;
    }
    
    let existing_ids: HashSet<String> = (0..model.row_count())
        .filter_map(|i| model.row_data(i).map(|item| get_id(&item)))
        .collect();
    
    for (id, item) in new_map {
        if !existing_ids.contains(&id) {
            model.push(item);
        }
    }
}
