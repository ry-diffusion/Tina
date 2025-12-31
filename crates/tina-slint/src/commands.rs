use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum Command {
    CreateAccount { id: String, name: String },
    StartAccount { account_id: String },
    StopAccount { account_id: String },
    SelectAccount { account_id: String },
    SelectChat { chat_jid: String },
    LoadMessages { account_id: String, chat_jid: String },
    SendMessage { account_id: String, to: String, content: String },
    RefreshChats,
    Shutdown,
}

pub type CommandSender = mpsc::Sender<Command>;
pub type CommandReceiver = mpsc::Receiver<Command>;

pub fn create_command_channel() -> (CommandSender, CommandReceiver) {
    mpsc::channel(256)
}
