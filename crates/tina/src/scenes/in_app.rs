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

    #[allow(dead_code)]
    pub async fn load_account_data(&self, account_id: &str) -> color_eyre::Result<()> {
        let _ = self.worker.list_chat_rows(account_id).await?;
        Ok(())
    }
}
