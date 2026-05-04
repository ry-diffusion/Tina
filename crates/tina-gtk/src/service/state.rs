// Per-account state for the service runtime.
//
// `selected: Arc<Mutex<Option<String>>>` was the single point of truth
// for "which account are commands targeting?" — workable while the UI
// only ever sees one account. To widen toward multi-account (paper-
// plane's `ClientManager` shape), per-account fields go into
// `AccountState`, the registry keys by `account_id`, and `active`
// remains as a focus pointer the UI can flip.
//
// Today `AccountState` is empty — the *existence* of an entry already
// means "this account has been initialised by the runtime". As features
// like per-account composer drafts or per-account selected chat get
// added, they slot in here without re-plumbing the runtime.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

#[derive(Default)]
pub(super) struct AccountState {}

#[derive(Default)]
pub(super) struct ServiceState {
    accounts: HashMap<String, AccountState>,
    /// Currently focused account. `Cmd`s without an explicit account_id
    /// resolve to this. Single-account today; the UI will start passing
    /// account_id through Cmd variants when it gains a sidebar account
    /// switcher, at which point handlers prefer that over `active`.
    active: Option<String>,
}

impl ServiceState {
    pub fn active_account(&self) -> Option<String> {
        self.active.clone()
    }

    /// Register `account_id` (creating an empty `AccountState` slot if
    /// it doesn't exist yet) and mark it as the focused account.
    pub fn set_active(&mut self, account_id: String) {
        self.accounts.entry(account_id.clone()).or_default();
        self.active = Some(account_id);
    }

    /// Drop the account from the registry. Clears `active` if it
    /// pointed at this account so a subsequent unscoped `Cmd` returns
    /// rather than acting on a removed account.
    #[allow(dead_code)]
    pub fn remove_account(&mut self, account_id: &str) {
        self.accounts.remove(account_id);
        if self.active.as_deref() == Some(account_id) {
            self.active = None;
        }
    }
}

pub(super) type SharedState = Arc<RwLock<ServiceState>>;
