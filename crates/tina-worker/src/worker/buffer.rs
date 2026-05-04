// `DirtyBuffer`: per-account accumulator for IPC events that benefit
// from being applied as a single SQLite transaction (messages/contacts/
// groups). Realtime events (Connected, QR, etc.) bypass it.

use std::collections::HashMap;

use tina_core::{ContactData, GroupData, MessageData};

/// Janela de flush do `DirtyBuffer`: durante sync, eventos bulk chegam
/// centenas por segundo. Acumular 100ms permite mesclar várias
/// `MessagesUpsert` num único `run_message_batch` (uma transação SQLite
/// ⇒ um fsync).
pub(super) const FLUSH_WINDOW: std::time::Duration = std::time::Duration::from_millis(100);

/// Threshold de itens acumulados (mensagens + contatos + grupos somados)
/// antes de forçar flush — evita acumular MB sem aplicar.
pub(super) const FLUSH_THRESHOLD: usize = 5000;

#[derive(Default)]
pub(super) struct DirtyBuffer {
    pub(super) messages: HashMap<String, Vec<MessageData>>,
    pub(super) contacts: HashMap<String, Vec<ContactData>>,
    pub(super) groups: HashMap<String, Vec<GroupData>>,
}

impl DirtyBuffer {
    pub(super) fn is_empty(&self) -> bool {
        self.messages.is_empty() && self.contacts.is_empty() && self.groups.is_empty()
    }
    pub(super) fn total_count(&self) -> usize {
        self.messages.values().map(|v| v.len()).sum::<usize>()
            + self.contacts.values().map(|v| v.len()).sum::<usize>()
            + self.groups.values().map(|v| v.len()).sum::<usize>()
    }
}
