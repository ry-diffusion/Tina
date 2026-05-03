use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, mpsc};

use tina_core::{ContactData, GroupData, IpcCommand, IpcEvent, MessageData};
use tina_db::{ChatRow, TinaDb};
use tina_ipc::{NanachiManager, SLOW_IPC_THRESHOLD};

use crate::error::Result;
use crate::events::WorkerEvent;

/// Janela de flush do `DirtyBuffer`: durante sync, eventos bulk chegam
/// centenas por segundo. Acumular 100ms permite mesclar várias `MessagesUpsert`
/// num único `run_message_batch` (uma transação SQLite ⇒ um fsync).
const FLUSH_WINDOW: Duration = Duration::from_millis(100);
/// Threshold de itens acumulados (mensagens + contatos + grupos somados)
/// antes de forçar flush — evita acumular MB sem aplicar.
const FLUSH_THRESHOLD: usize = 5000;

/// Eventos *bulkables* acumulam por account_id em cima do dispatcher.
/// Eventos *realtime* (Connected, QR, etc.) bypass direto pra UI.
#[derive(Default)]
struct DirtyBuffer {
    messages: HashMap<String, Vec<MessageData>>,
    contacts: HashMap<String, Vec<ContactData>>,
    groups: HashMap<String, Vec<GroupData>>,
}

impl DirtyBuffer {
    fn is_empty(&self) -> bool {
        self.messages.is_empty() && self.contacts.is_empty() && self.groups.is_empty()
    }
    fn total_count(&self) -> usize {
        self.messages.values().map(|v| v.len()).sum::<usize>()
            + self.contacts.values().map(|v| v.len()).sum::<usize>()
            + self.groups.values().map(|v| v.len()).sum::<usize>()
    }
}

pub struct TinaWorker {
    db: Arc<TinaDb>,
    nanachi: Arc<RwLock<NanachiManager>>,
    event_tx: mpsc::Sender<WorkerEvent>,
    event_rx: Option<mpsc::Receiver<WorkerEvent>>,
    /// Chat atualmente em foco na UI. Quando definido, mensagens novas para
    /// ele saem como `MessagesAppended` além do `ChatsUpserted` padrão.
    active_chat: Arc<RwLock<Option<(String /*account*/, String /*chat_id*/)>>>,
}

impl TinaWorker {
    pub async fn new(nanachi_dir: PathBuf) -> Result<Self> {
        let db = TinaDb::new().await?;
        let nanachi = NanachiManager::new(nanachi_dir);
        let (event_tx, event_rx) = mpsc::channel(5000);
        Ok(Self {
            db: Arc::new(db),
            nanachi: Arc::new(RwLock::new(nanachi)),
            event_tx,
            event_rx: Some(event_rx),
            active_chat: Arc::new(RwLock::new(None)),
        })
    }

    pub fn take_event_receiver(&mut self) -> Option<mpsc::Receiver<WorkerEvent>> {
        self.event_rx.take()
    }

    pub async fn start(&self) -> Result<()> {
        let mut nanachi = self.nanachi.write().await;
        nanachi.start().await?;
        let ipc_rx = nanachi.take_event_receiver();
        let outstanding = nanachi.outstanding_handle();

        if let Some(rx) = ipc_rx {
            let db = self.db.clone();
            let event_tx = self.event_tx.clone();
            let active_chat = self.active_chat.clone();
            tokio::spawn(dispatcher_loop(db, event_tx, active_chat, outstanding, rx));
        }
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        let mut nanachi = self.nanachi.write().await;
        nanachi.stop().await?;
        Ok(())
    }

    // ---- Account management (delegado a tina-db) ----

    pub async fn create_account(
        &self,
        account_id: &str,
        name: Option<&str>,
    ) -> Result<tina_db::Account> {
        Ok(self.db.create_account(account_id, name).await?)
    }

    pub async fn list_accounts(&self) -> Result<Vec<tina_db::Account>> {
        Ok(self.db.list_accounts().await?)
    }

    pub async fn delete_account(&self, account_id: &str) -> Result<()> {
        Ok(self.db.delete_account(account_id).await?)
    }

    pub async fn start_account(&self, account_id: &str) -> Result<()> {
        let _ = self.db.get_account(account_id).await?;
        let nanachi = self.nanachi.read().await;
        nanachi
            .send_command(IpcCommand::StartAccount {
                account_id: account_id.to_string(),
            })
            .await?;
        Ok(())
    }

    pub async fn stop_account(&self, account_id: &str) -> Result<()> {
        let nanachi = self.nanachi.read().await;
        nanachi
            .send_command(IpcCommand::StopAccount {
                account_id: account_id.to_string(),
            })
            .await?;
        Ok(())
    }

    pub async fn logout_account(&self, account_id: &str) -> Result<()> {
        let nanachi = self.nanachi.read().await;
        nanachi
            .send_command(IpcCommand::Logout {
                account_id: account_id.to_string(),
            })
            .await?;
        Ok(())
    }

    /// Re-pesca tudo que o whatsmeow já tem em cache (contatos, grupos,
    /// newsletters) e emite eventos de upsert. Cura nomes faltando sem
    /// precisar de re-pareamento.
    pub async fn reconcile_account(&self, account_id: &str) -> Result<()> {
        let nanachi = self.nanachi.read().await;
        nanachi
            .send_command(IpcCommand::Reconcile {
                account_id: account_id.to_string(),
            })
            .await?;
        Ok(())
    }

    pub async fn send_message(&self, account_id: &str, to: &str, content: &str) -> Result<()> {
        let nanachi = self.nanachi.read().await;
        nanachi
            .send_command(IpcCommand::SendMessage {
                account_id: account_id.to_string(),
                to: to.to_string(),
                content: content.to_string(),
            })
            .await?;
        Ok(())
    }

    /// Solicita download de mídia. Faz dedup local primeiro: se outra
    /// mensagem com o mesmo sha256 já tem `media_path`, reaproveita esse
    /// caminho sem chamar o nanachi.
    pub async fn download_media(&self, account_id: &str, message_id: &str) -> Result<()> {
        if let Some(row) = self
            .db
            .get_message_rows_by_ids(account_id, &[message_id.to_string()])
            .await?
            .into_iter()
            .next()
        {
            if let Some(path) = row.media_path.as_deref() {
                if std::path::Path::new(path).exists() {
                    let _ = self
                        .event_tx
                        .send(WorkerEvent::MediaReady {
                            account_id: account_id.to_string(),
                            affected_message_ids: vec![message_id.to_string()],
                            path: path.to_string(),
                            mimetype: row.media_mimetype.clone(),
                        })
                        .await;
                    return Ok(());
                }
            }
            if let Some(sha) = row.media_sha256.as_deref() {
                if let Some(existing_path) =
                    self.db.find_existing_media_path(account_id, sha).await?
                {
                    let affected = self
                        .db
                        .apply_media_downloaded(
                            account_id,
                            message_id,
                            &existing_path,
                            Some(sha),
                            row.media_mimetype.as_deref(),
                        )
                        .await?;
                    let _ = self
                        .event_tx
                        .send(WorkerEvent::MediaReady {
                            account_id: account_id.to_string(),
                            affected_message_ids: affected,
                            path: existing_path,
                            mimetype: row.media_mimetype.clone(),
                        })
                        .await;
                    return Ok(());
                }
            }
        }

        // Marca como downloading pra UI exibir spinner enquanto IPC volta.
        self.db
            .set_media_status(account_id, message_id, "downloading")
            .await?;
        let nanachi = self.nanachi.read().await;
        nanachi
            .send_command(IpcCommand::DownloadMedia {
                account_id: account_id.to_string(),
                message_id: message_id.to_string(),
            })
            .await?;
        Ok(())
    }

    // ---- Chat-list / messages para a UI ----

    pub async fn list_chat_rows(&self, account_id: &str) -> Result<Vec<ChatRow>> {
        Ok(self.db.list_chat_rows(account_id).await?)
    }

    pub async fn get_messages(
        &self,
        account_id: &str,
        chat_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<tina_db::Message>> {
        Ok(self
            .db
            .get_messages_by_chat(account_id, chat_id, limit, offset)
            .await?)
    }

    /// Mensagens com `sender_name` já resolvido — pra renderização da janela.
    pub async fn get_message_rows(
        &self,
        account_id: &str,
        chat_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<tina_db::MessageRow>> {
        Ok(self
            .db
            .get_message_rows_by_chat(account_id, chat_id, limit, offset)
            .await?)
    }

    pub async fn get_chat(&self, account_id: &str, chat_id: &str) -> Result<Option<tina_db::Chat>> {
        Ok(self.db.get_chat(account_id, chat_id).await?)
    }

    pub async fn get_chat_row(&self, account_id: &str, chat_id: &str) -> Result<Option<ChatRow>> {
        let rows = self
            .db
            .get_chat_rows(account_id, &[chat_id.to_string()])
            .await?;
        Ok(rows.into_iter().next())
    }

    /// UI define o chat atualmente aberto (ou None quando volta pra lista).
    /// Enquanto definido, novas mensagens daquele chat saem como
    /// `MessagesAppended` para renderização incremental.
    pub async fn set_active_chat(&self, account_id: &str, chat_id: Option<&str>) {
        let mut guard = self.active_chat.write().await;
        *guard = chat_id.map(|c| (account_id.to_string(), c.to_string()));
    }
}

// ============================================================================
// IPC event pipeline
// ============================================================================

/// Dispatcher: dono único do `DirtyBuffer` + timer de flush + correlação
/// de CommandResult. IPC reader nunca espera DB; eventos bulk acumulam até
/// flush. Eventos realtime (Connected, QR, etc.) processam inline.
async fn dispatcher_loop(
    db: Arc<TinaDb>,
    event_tx: mpsc::Sender<WorkerEvent>,
    active_chat: Arc<RwLock<Option<(String, String)>>>,
    outstanding: Arc<std::sync::Mutex<HashMap<String, tina_ipc::CommandTiming>>>,
    mut raw_rx: mpsc::Receiver<String>,
) {
    let mut buffer = DirtyBuffer::default();
    let mut deadline: Option<tokio::time::Instant> = None;

    loop {
        // Sleep condicional: só fica pendente se buffer tem conteúdo.
        let timer = async {
            if let Some(t) = deadline {
                tokio::time::sleep_until(t).await;
            } else {
                std::future::pending::<()>().await;
            }
        };

        tokio::select! {
            biased;
            line = raw_rx.recv() => {
                let Some(line) = line else { break };
                let Some(event) = NanachiManager::parse_event(&line) else { continue };

                // Round-trip de comandos.
                if let IpcEvent::CommandResult { ref command_id, .. } = event {
                    if let Ok(mut map) = outstanding.lock() {
                        if let Some(t) = map.remove(command_id) {
                            let rtt = t.sent_at.elapsed();
                            if rtt > SLOW_IPC_THRESHOLD {
                                tracing::warn!("🐌 IPC round-trip {} → {:?}", t.kind, rtt);
                            } else {
                                tracing::trace!("IPC round-trip {} → {:?}", t.kind, rtt);
                            }
                        }
                    }
                }

                let started = Instant::now();
                let kind = event_kind(&event);

                // Bulkables → buffer; resto → realtime inline.
                let bulked = match event {
                    IpcEvent::MessagesUpsert { account_id, messages } => {
                        if !messages.is_empty() {
                            buffer.messages.entry(account_id).or_default().extend(messages);
                        }
                        true
                    }
                    IpcEvent::ContactsUpsert { account_id, contacts } => {
                        if !contacts.is_empty() {
                            buffer.contacts.entry(account_id).or_default().extend(contacts);
                        }
                        true
                    }
                    IpcEvent::GroupsUpsert { account_id, groups } => {
                        if !groups.is_empty() {
                            buffer.groups.entry(account_id).or_default().extend(groups);
                        }
                        true
                    }
                    other => {
                        if let Err(e) = handle_realtime_event(&db, &event_tx, other).await {
                            tracing::error!("realtime handler error: {}", e);
                        }
                        false
                    }
                };

                if bulked {
                    if deadline.is_none() && !buffer.is_empty() {
                        deadline = Some(tokio::time::Instant::now() + FLUSH_WINDOW);
                    }
                    if buffer.total_count() >= FLUSH_THRESHOLD {
                        if let Err(e) = flush(&db, &event_tx, &active_chat, &mut buffer).await {
                            tracing::error!("flush error: {}", e);
                        }
                        deadline = None;
                    }
                }

                let elapsed = started.elapsed();
                if elapsed > SLOW_IPC_THRESHOLD {
                    tracing::warn!(
                        "🐌 evento {} levou {:?} (DB ou pipeline lento)",
                        kind,
                        elapsed
                    );
                }
            }
            _ = timer, if deadline.is_some() => {
                if let Err(e) = flush(&db, &event_tx, &active_chat, &mut buffer).await {
                    tracing::error!("flush error: {}", e);
                }
                deadline = None;
            }
        }
    }

    // Drain final ao fechar.
    if !buffer.is_empty() {
        let _ = flush(&db, &event_tx, &active_chat, &mut buffer).await;
    }
}

/// Aplica todo o buffer numa transação coletiva por account_id,
/// emitindo um único `ChatsUpserted` por account no final.
async fn flush(
    db: &TinaDb,
    event_tx: &mpsc::Sender<WorkerEvent>,
    active_chat: &Arc<RwLock<Option<(String, String)>>>,
    buffer: &mut DirtyBuffer,
) -> Result<()> {
    let started = Instant::now();
    let count_msgs: usize = buffer.messages.values().map(|v| v.len()).sum();
    let count_contacts: usize = buffer.contacts.values().map(|v| v.len()).sum();
    let count_groups: usize = buffer.groups.values().map(|v| v.len()).sum();

    let mut affected: HashMap<String, HashSet<String>> = HashMap::new();
    let active = active_chat.read().await.clone();

    // 1. Mensagens (criam chats, registram senders).
    let messages = std::mem::take(&mut buffer.messages);
    for (account_id, msgs) in messages {
        let active_chat_for_account = active
            .as_ref()
            .filter(|(a, _)| a == &account_id)
            .map(|(_, c)| c.clone());

        let inputs: Vec<tina_db::MessageBatchInput<'_>> = msgs
            .iter()
            .map(|m| tina_db::MessageBatchInput {
                message_id: &m.message_id,
                chat_jid: &m.chat_jid,
                sender_jid: if m.sender_jid.is_empty() {
                    None
                } else {
                    Some(m.sender_jid.as_str())
                },
                content: m.content.as_deref(),
                message_type: &m.message_type,
                timestamp: m.timestamp,
                is_from_me: m.is_from_me,
                raw_json: m.raw_json.as_deref(),
                media_mimetype: m.media_mimetype.as_deref(),
                media_filename: m.media_filename.as_deref(),
                media_duration_secs: m.media_duration_secs,
                media_width: m.media_width,
                media_height: m.media_height,
                media_size_bytes: m.media_size_bytes,
                media_sha256: m.media_sha256.as_deref(),
            })
            .collect();

        let res = db
            .run_message_batch(&account_id, active_chat_for_account.as_deref(), &inputs)
            .await?;

        affected
            .entry(account_id.clone())
            .or_default()
            .extend(res.affected_chat_ids);

        // Diagnostics: surface why a flush did or did not emit
        // MessagesAppended. Useful when chat_id resolution mismatches the
        // UI's active chat (e.g., LID vs PN).
        match (
            active_chat_for_account.as_deref(),
            res.active_chat_message_ids.is_empty(),
        ) {
            (Some(chat_id), false) => {
                tracing::info!(
                    chat = chat_id,
                    count = res.active_chat_message_ids.len(),
                    "dispatcher: emitting MessagesAppended",
                );
                let rows = db
                    .get_message_rows_by_ids(&account_id, &res.active_chat_message_ids)
                    .await?;
                if !rows.is_empty() {
                    let _ = event_tx
                        .send(WorkerEvent::MessagesAppended {
                            account_id: account_id.clone(),
                            chat_id: chat_id.to_string(),
                            messages: rows,
                        })
                        .await;
                }
            }
            (Some(chat_id), true) => {
                // WARN so it shows up in default log levels — this is the
                // smoking-gun symptom of an active_chat / chat_id mismatch
                // (the user reported new messages not rendering live).
                tracing::warn!(
                    active_chat = chat_id,
                    msg_inputs = inputs.len(),
                    "dispatcher: flush had active_chat but no message matched (alias mismatch?)",
                );
            }
            (None, _) => {
                tracing::debug!(
                    affected = "tracked",
                    msg_inputs = inputs.len(),
                    "dispatcher: flush had no active chat",
                );
            }
        }
    }

    // 2. Contatos.
    let contacts = std::mem::take(&mut buffer.contacts);
    for (account_id, list) in contacts {
        let chat_ids = process_contacts(db, &account_id, list).await?;
        affected
            .entry(account_id.clone())
            .or_default()
            .extend(chat_ids);
    }

    // 3. Grupos / newsletters.
    let groups = std::mem::take(&mut buffer.groups);
    for (account_id, list) in groups {
        let chat_ids = process_groups(db, &account_id, list).await?;
        affected
            .entry(account_id.clone())
            .or_default()
            .extend(chat_ids);
    }

    // 4. Um único ChatsUpserted por account no fim.
    for (account_id, chat_ids) in affected {
        if chat_ids.is_empty() {
            continue;
        }
        let ids: Vec<String> = chat_ids.into_iter().collect();
        match db.get_chat_rows(&account_id, &ids).await {
            Ok(rows) if !rows.is_empty() => {
                tracing::debug!(
                    "🔄 flush → {} chat(s) emitidos para {}",
                    rows.len(),
                    account_id
                );
                let _ = event_tx
                    .send(WorkerEvent::ChatsUpserted {
                        account_id,
                        rows,
                    })
                    .await;
            }
            Ok(_) => {}
            Err(e) => tracing::error!("get_chat_rows failed: {}", e),
        }
    }

    let elapsed = started.elapsed();
    // Flush é trabalho pesado; threshold mais frouxo que o resto do IPC.
    const SLOW_FLUSH_THRESHOLD: Duration = Duration::from_millis(200);
    if elapsed > SLOW_FLUSH_THRESHOLD {
        tracing::warn!(
            "🐌 flush levou {:?} (msgs={}, contacts={}, groups={})",
            elapsed,
            count_msgs,
            count_contacts,
            count_groups
        );
    } else if elapsed > SLOW_IPC_THRESHOLD {
        tracing::debug!(
            "flush {:?} (msgs={}, contacts={}, groups={})",
            elapsed,
            count_msgs,
            count_contacts,
            count_groups
        );
    }
    Ok(())
}

fn event_kind(e: &IpcEvent) -> &'static str {
    match e {
        IpcEvent::Ready { .. } => "Ready",
        IpcEvent::QrCode { .. } => "QrCode",
        IpcEvent::PairingCode { .. } => "PairingCode",
        IpcEvent::Connected { .. } => "Connected",
        IpcEvent::Disconnected { .. } => "Disconnected",
        IpcEvent::LoggedOut { .. } => "LoggedOut",
        IpcEvent::ContactsUpsert { .. } => "ContactsUpsert",
        IpcEvent::GroupsUpsert { .. } => "GroupsUpsert",
        IpcEvent::MessagesUpsert { .. } => "MessagesUpsert",
        IpcEvent::HistorySyncComplete { .. } => "HistorySyncComplete",
        IpcEvent::ReconcileProgress { .. } => "ReconcileProgress",
        IpcEvent::Error { .. } => "Error",
        IpcEvent::MediaDownloadProgress { .. } => "MediaDownloadProgress",
        IpcEvent::MediaDownloaded { .. } => "MediaDownloaded",
        IpcEvent::MediaDownloadFailed { .. } => "MediaDownloadFailed",
        IpcEvent::CommandResult { .. } => "CommandResult",
    }
}

/// Processa eventos *realtime* (Ready, QR, Connected, Disconnected, etc.).
/// Esses não passam pelo DirtyBuffer — vão direto pra UI ou DB simples.
async fn handle_realtime_event(
    db: &TinaDb,
    event_tx: &mpsc::Sender<WorkerEvent>,
    event: IpcEvent,
) -> Result<()> {
    match event {
        IpcEvent::Ready { account_id } => {
            if account_id.is_empty() {
                let _ = event_tx.send(WorkerEvent::NanachiReady).await;
            } else {
                let _ = event_tx
                    .send(WorkerEvent::AccountReady { account_id })
                    .await;
            }
        }

        IpcEvent::QrCode { account_id, qr } => {
            let _ = event_tx.send(WorkerEvent::QrCode { account_id, qr }).await;
        }

        IpcEvent::PairingCode { account_id, code } => {
            tracing::info!("Pairing code for {}: {}", account_id, code);
        }

        IpcEvent::Connected {
            account_id,
            phone_number,
            jid,
        } => {
            db.save_account_identity(&account_id, phone_number.as_deref(), jid.as_deref())
                .await?;
            let _ = event_tx
                .send(WorkerEvent::Connected {
                    account_id,
                    phone_number,
                })
                .await;
        }

        IpcEvent::Disconnected { account_id, reason } => {
            let _ = event_tx
                .send(WorkerEvent::Disconnected { account_id, reason })
                .await;
        }

        IpcEvent::LoggedOut { account_id } => {
            db.clear_account_identity(&account_id).await?;
            let _ = event_tx.send(WorkerEvent::LoggedOut { account_id }).await;
        }

        // Bulkables são consumidos pelo dispatcher antes de chegarem aqui;
        // se aparecerem no realtime handler é bug do roteamento.
        IpcEvent::ContactsUpsert { .. }
        | IpcEvent::GroupsUpsert { .. }
        | IpcEvent::MessagesUpsert { .. } => {
            tracing::error!("bulk event reached realtime handler — routing bug");
        }

        IpcEvent::ReconcileProgress {
            account_id,
            stage,
            current,
            total,
            indeterminate,
        } => {
            let _ = event_tx
                .send(WorkerEvent::ReconcileProgress {
                    account_id,
                    stage,
                    current,
                    total,
                    indeterminate,
                })
                .await;
        }

        IpcEvent::HistorySyncComplete {
            account_id,
            messages_count,
        } => {
            let _ = event_tx
                .send(WorkerEvent::HistorySyncComplete {
                    account_id,
                    messages_count,
                })
                .await;
        }

        IpcEvent::Error { account_id, error } => {
            let _ = event_tx
                .send(WorkerEvent::Error { account_id, error })
                .await;
        }

        IpcEvent::MediaDownloadProgress {
            account_id,
            message_id,
            current,
            total,
        } => {
            let _ = event_tx
                .send(WorkerEvent::MediaDownloadProgress {
                    account_id,
                    message_id,
                    current,
                    total,
                })
                .await;
        }

        IpcEvent::MediaDownloaded {
            account_id,
            message_id,
            path,
            sha256,
            mimetype,
        } => {
            // Persiste no DB (com dedup pra todas as mensagens com mesmo
            // sha256) e emite MediaReady com a lista completa de IDs.
            let affected = db
                .apply_media_downloaded(
                    &account_id,
                    &message_id,
                    &path,
                    sha256.as_deref(),
                    mimetype.as_deref(),
                )
                .await
                .unwrap_or_else(|e| {
                    tracing::error!("apply_media_downloaded: {e}");
                    vec![message_id.clone()]
                });
            let _ = event_tx
                .send(WorkerEvent::MediaReady {
                    account_id,
                    affected_message_ids: affected,
                    path,
                    mimetype,
                })
                .await;
        }

        IpcEvent::MediaDownloadFailed {
            account_id,
            message_id,
            error,
        } => {
            let _ = db
                .set_media_status(&account_id, &message_id, "failed")
                .await;
            let _ = event_tx
                .send(WorkerEvent::MediaDownloadFailed {
                    account_id,
                    message_id,
                    error,
                })
                .await;
        }

        IpcEvent::CommandResult {
            command_id,
            success,
            error,
            ..
        } => {
            if !success {
                tracing::warn!(
                    "Command {} failed: {}",
                    command_id,
                    error.unwrap_or_else(|| "<no error>".to_string())
                );
            }
        }
    }
    Ok(())
}

/// Pura: aplica contatos em UMA transação e devolve os DM chats afetados.
async fn process_contacts(
    db: &TinaDb,
    account_id: &str,
    contacts: Vec<ContactData>,
) -> Result<HashSet<String>> {
    if contacts.is_empty() {
        return Ok(HashSet::new());
    }

    let inputs: Vec<tina_db::ContactBatchInput<'_>> = contacts
        .iter()
        .map(|c| tina_db::ContactBatchInput {
            jid: &c.jid,
            lid: c.lid.as_deref(),
            phone_number: c.phone_number.as_deref(),
            push_name: c.notify.as_deref(),
            contact_name: c.name.as_deref(),
            verified_name: c.verified_name.as_deref(),
            avatar_url: c.img_url.as_deref(),
            status: c.status.as_deref(),
        })
        .collect();

    let aliases = db.run_contacts_batch(account_id, &inputs).await?;

    // Lookup bulk de DM chats afetados (read-only, fora da transação).
    const CHUNK: usize = 500;
    let mut affected: HashSet<String> = HashSet::new();
    let alias_refs: Vec<&str> = aliases.iter().map(|s| s.as_str()).collect();
    for chunk in alias_refs.chunks(CHUNK) {
        let ids = db.find_dm_chat_ids_for_aliases(account_id, chunk).await?;
        affected.extend(ids);
    }
    Ok(affected)
}

/// Pura: aplica grupos/newsletters em UMA transação e devolve chats afetados.
async fn process_groups(
    db: &TinaDb,
    account_id: &str,
    groups: Vec<GroupData>,
) -> Result<HashSet<String>> {
    if groups.is_empty() {
        return Ok(HashSet::new());
    }
    // participants_json + participant_jids precisam viver pelo escopo da chamada.
    let mut participants_json: Vec<Option<String>> = Vec::with_capacity(groups.len());
    let mut participant_id_storage: Vec<Vec<String>> = Vec::with_capacity(groups.len());
    for g in &groups {
        participants_json.push(serde_json::to_string(&g.participants).ok());
        participant_id_storage.push(g.participants.iter().map(|p| p.id.clone()).collect());
    }
    // Refs depois que os Strings já estão armazenados.
    let participant_refs: Vec<Vec<&str>> = participant_id_storage
        .iter()
        .map(|v| v.iter().map(|s| s.as_str()).collect())
        .collect();

    let inputs: Vec<tina_db::GroupBatchInput<'_>> = groups
        .iter()
        .enumerate()
        .map(|(i, g)| tina_db::GroupBatchInput {
            jid: &g.jid,
            subject: g.subject.as_deref(),
            owner: g.owner.as_deref(),
            description: g.description.as_deref(),
            participants_json: participants_json[i].as_deref(),
            participant_jids: participant_refs[i].as_slice(),
        })
        .collect();

    let affected = db.run_groups_batch(account_id, &inputs).await?;
    Ok(affected.into_iter().collect())
}
