use crate::Scene;
use tina_worker::Account;

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
}
