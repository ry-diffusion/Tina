use crate::Scene;
use tina_worker::{Account, ChatRow};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum UIMessage {
    Quit,
    Initialize,
    CreateAccount,
    LoginRequested(String),
    ShowScene(Scene),
    ShowQrLogin,
    ShowAccountSelection(Vec<Account>),
    ShowSyncing,
    ShowInApp,
    ShowError(String),
    QrCodeReceived(String),
    AccountSelected(String),

    /// Pede ao worker um snapshot inicial completo da lista de chats.
    LoadChats,

    /// Worker entregou linhas resolvidas para a lista; UI faz upsert por chat_id
    /// e re-ordena.
    ApplyChatsUpserted(Vec<ChatRow>),

    /// UI seleciona/desseleciona chat aberto. None = lista. Um Some destino
    /// faz o worker passar a empurrar `MessagesAppended` para esse chat.
    SetActiveChat(Option<String>),

    /// Botão Reparar: dispara reconcile (whatsmeow → tina) e reseta a UI.
    RepairRequested,
}
