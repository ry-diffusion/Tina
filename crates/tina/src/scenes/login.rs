use color_eyre::eyre::{Result as EyreResult, eyre};
use slint::{ComponentHandle, SharedString, Weak};
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

use crate::Tina;
use crate::state::UIMessage;
use tina_worker::{Account, TinaWorker};

type UiSendError = tokio::sync::mpsc::error::SendError<UIMessage>;

#[derive(Clone)]
pub struct LoginScene {
    ui_handle: Weak<Tina>,
    worker: Arc<TinaWorker>,
    tx: UnboundedSender<UIMessage>,
}

impl LoginScene {
    pub fn new(
        ui_handle: Weak<Tina>,
        worker: Arc<TinaWorker>,
        tx: UnboundedSender<UIMessage>,
    ) -> Self {
        let scene = Self {
            ui_handle,
            worker,
            tx,
        };

        scene.register_callbacks();

        scene
    }

    /// Check if accounts exist and transition accordingly
    pub async fn check_and_transition(&self) -> EyreResult<()> {
        let accounts = self.refresh_account_list().await?;

        if accounts.is_empty() {
            self.create_account_and_login().await?
        } else if accounts.len() == 1 {
            self.start_login(&accounts[0]).await?
        } else if let Some(first) = accounts.first() {
            self.tx
                .send(UIMessage::AccountSelected(first.id.clone()))
                .map_err(|e: UiSendError| eyre!(e))?;
        }

        Ok(())
    }

    pub async fn handle_login_request(&self, account_id: String) -> EyreResult<()> {
        self.start_login_by_id(&account_id).await
    }

    pub async fn handle_create_account(&self) -> EyreResult<()> {
        self.create_account_and_login().await
    }

    fn register_callbacks(&self) {
        if let Some(ui) = self.ui_handle.upgrade() {
            let am = ui.global::<crate::AccountManagement>();

            am.on_select_account({
                let tx = self.tx.clone();

                move |value: SharedString| {
                    let id = value.to_string();
                    let _ = tx.send(UIMessage::AccountSelected(id));
                }
            });

            am.on_login_account({
                let tx = self.tx.clone();

                move |value: SharedString| {
                    let id = value.to_string();
                    if id.is_empty() {
                        return;
                    }
                    let _ = tx.send(UIMessage::LoginRequested(id));
                }
            });

            am.on_create_account({
                let tx = self.tx.clone();

                move || {
                    let _ = tx.send(UIMessage::CreateAccount);
                }
            });
        }
    }

    async fn refresh_account_list(&self) -> EyreResult<Vec<Account>> {
        let accounts = self.worker.list_accounts().await?;
        self.tx
            .send(UIMessage::ShowAccountSelection(accounts.clone()))
            .map_err(|e: UiSendError| eyre!(e))?;
        Ok(accounts)
    }

    async fn create_account_and_login(&self) -> EyreResult<()> {
        let account_id = format!("tina-{}", Uuid::new_v4().simple());
        self.worker.create_account(&account_id, None).await?;

        let accounts = self.refresh_account_list().await?;
        self.start_login_by_id(
            accounts
                .iter()
                .find(|acc| acc.id == account_id)
                .ok_or_else(|| eyre!("Account {} not found after creation", account_id))?
                .id
                .as_str(),
        )
        .await
    }

    async fn start_login_by_id(&self, account_id: &str) -> EyreResult<()> {
        let accounts = self.worker.list_accounts().await?;
        let account = accounts
            .into_iter()
            .find(|acc| acc.id == account_id)
            .ok_or_else(|| eyre!("Account {} not found", account_id))?;
        self.start_login(&account).await
    }

    async fn start_login(&self, account: &Account) -> EyreResult<()> {
        self.tx
            .send(UIMessage::AccountSelected(account.id.clone()))
            .map_err(|e: UiSendError| eyre!(e))?;

        if account.auth_state.is_some() {
            self.tx
                .send(UIMessage::ShowInApp)
                .map_err(|e: UiSendError| eyre!(e))?;
        } else {
            self.tx
                .send(UIMessage::ShowQrLogin)
                .map_err(|e: UiSendError| eyre!(e))?;
        }

        if let Err(e) = self.worker.start_account(&account.id).await {
            let _ = self.tx.send(UIMessage::ShowError(format!(
                "Failed to start account {}: {}",
                account.id, e
            )));
            return Err(e.into());
        }

        Ok(())
    }
}
