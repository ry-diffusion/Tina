use crate::app::Message;
use crate::worker_bridge::WorkerHandle;
use std::sync::Arc;

/// Async command scheduler that uses Task::perform to avoid blocking the UI
pub struct CommandScheduler {
    worker: Option<Arc<tina_worker::TinaWorker>>,
}

impl CommandScheduler {
    pub fn new(handle: Option<WorkerHandle>) -> Self {
        Self {
            worker: handle.map(|h| h.worker()),
        }
    }

    /// List all accounts and return the result
    pub fn list_accounts(&self) -> iced::Task<Message> {
        let worker = self.worker.clone();

        iced::Task::perform(
            async move {
                tracing::info!("Listing accounts asynchronously");

                let result = match worker {
                    Some(w) => w.list_accounts().await.map_err(|e| e.to_string()),
                    None => Err("Worker not initialized".to_string()),
                };

                match &result {
                    Ok(accounts) => {
                        tracing::info!(count = accounts.len(), "Accounts listed successfully");
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to list accounts");
                    }
                }

                result
            },
            |result| Message::AccountsListResult(result),
        )
    }

    /// Create a new account
    pub fn create_account(&self, id: String, name: Option<String>) -> iced::Task<Message> {
        let worker = self.worker.clone();

        iced::Task::perform(
            async move {
                tracing::info!(account_id = %id, name = ?name, "Creating account asynchronously");

                let result = match worker {
                    Some(w) => w.create_account(&id, name.as_deref()).await.map_err(|e| e.to_string()),
                    None => Err("Worker not initialized".to_string()),
                };

                match &result {
                    Ok(_) => {
                        tracing::info!("Account created successfully");
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to create account");
                    }
                }

                result.map(|_| ())
            },
            |result| Message::AccountCreatedResult(result),
        )
    }

    /// Start an account (connect to WhatsApp)
    pub fn start_account(&self, account_id: String) -> iced::Task<Message> {
        let worker = self.worker.clone();

        iced::Task::perform(
            async move {
                tracing::info!(account_id = %account_id, "Starting account asynchronously");

                let result = match worker {
                    Some(w) => w.start_account(&account_id).await.map_err(|e| e.to_string()),
                    None => Err("Worker not initialized".to_string()),
                };

                match &result {
                    Ok(_) => {
                        tracing::info!("Account started successfully");
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to start account");
                    }
                }

                result.map(|_| ())
            },
            |result| Message::AccountStartedResult(result),
        )
    }

    /// Load chats for an account
    pub fn load_chats(&self, account_id: String) -> iced::Task<Message> {
        let worker = self.worker.clone();

        iced::Task::perform(
            async move {
                tracing::info!(account_id = %account_id, "Loading chats asynchronously");

                let result = match worker {
                    Some(w) => w.get_chats_basic(&account_id).await.map_err(|e| e.to_string()),
                    None => Err("Worker not initialized".to_string()),
                };

                match &result {
                    Ok(chats) => {
                        tracing::info!(count = chats.len(), "Chats loaded successfully");
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to load chats");
                    }
                }

                result
            },
            |result| Message::ChatsLoadedResult(result),
        )
    }

    /// Load chat previews (with last messages)
    pub fn load_previews(&self, account_id: String) -> iced::Task<Message> {
        let worker = self.worker.clone();

        iced::Task::perform(
            async move {
                tracing::info!(account_id = %account_id, "Loading previews asynchronously");

                let result = match worker {
                    Some(w) => w.get_chat_previews(&account_id).await.map_err(|e| e.to_string()),
                    None => Err("Worker not initialized".to_string()),
                };

                match &result {
                    Ok(previews) => {
                        tracing::info!(count = previews.len(), "Previews loaded successfully");
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to load previews");
                    }
                }

                result
            },
            |result| Message::PreviewsLoadedResult(result),
        )
    }

    /// Load messages for a chat
    pub fn load_messages(
        &self,
        account_id: String,
        chat_jid: String,
        limit: i64,
        offset: i64,
    ) -> iced::Task<Message> {
        let worker = self.worker.clone();

        iced::Task::perform(
            async move {
                tracing::info!(
                    account_id = %account_id,
                    chat_jid = %chat_jid,
                    limit,
                    offset,
                    "Loading messages asynchronously"
                );

                let result = match worker {
                    Some(w) => w.get_chat_messages(&account_id, &chat_jid, limit, offset).await.map_err(|e| e.to_string()),
                    None => Err("Worker not initialized".to_string()),
                };

                match &result {
                    Ok(messages) => {
                        tracing::info!(count = messages.len(), "Messages loaded successfully");
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to load messages");
                    }
                }

                result
            },
            |result| Message::MessagesLoadedResult(result),
        )
    }

    /// Send a message
    pub fn send_message(
        &self,
        account_id: String,
        to: String,
        content: String,
    ) -> iced::Task<Message> {
        let worker = self.worker.clone();

        iced::Task::perform(
            async move {
                tracing::info!(
                    account_id = %account_id,
                    to = %to,
                    content_len = content.len(),
                    "Sending message asynchronously"
                );

                let result = match worker {
                    Some(w) => w.send_message(&account_id, &to, &content).await.map_err(|e| e.to_string()),
                    None => Err("Worker not initialized".to_string()),
                };

                match &result {
                    Ok(_) => {
                        tracing::info!("Message sent successfully");
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to send message");
                    }
                }

                result
            },
            |result| Message::MessageSentResult(result),
        )
    }
}
