use slint::Weak;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;

use crate::Tina;
use crate::state::UIMessage;
use tina_worker::TinaWorker;

#[derive(Clone)]
pub struct InAppScene {
    _ui_handle: Weak<Tina>,
    worker: Arc<TinaWorker>,
    _tx: UnboundedSender<UIMessage>,
}

impl InAppScene {
    #[allow(dead_code)]
    pub fn new(
        ui_handle: Weak<Tina>,
        worker: Arc<TinaWorker>,
        tx: UnboundedSender<UIMessage>,
    ) -> Self {
        Self {
            _ui_handle: ui_handle,
            worker,
            _tx: tx,
        }
    }

    /// Load chats and messages for the selected account
    #[allow(dead_code)]
    pub async fn load_account_data(&self, account_id: &str) -> color_eyre::Result<()> {
        let _contacts = self.worker.get_contacts(account_id).await?;
        let _chats = self.worker.get_chats(account_id).await?;

        tracing::info!("Loaded contacts and chats for {}", account_id);

        Ok(())
    }
}
