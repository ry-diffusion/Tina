// Composer-side mention handling: refreshing the popover when the
// worker hands us a new candidate list, plus rebuilding existing
// bubbles whose `@<digits>` chips just got names.

use tina_db::MentionCandidate;

use super::super::model::ChatTab;

impl ChatTab {
    pub(in crate::components::chat_tab) fn handle_mention_candidates_loaded(
        &mut self,
        candidates: Vec<MentionCandidate>,
    ) {
        // Inventory is the source of truth for the renderer; the
        // ChatArea routing handler already pushed the same list in
        // before forwarding here. We re-publish to be defensive
        // against future direct callers — `set_candidates` is
        // idempotent.
        self.mentions.set_candidates(&self.chat_id, &candidates);

        // Refresh the live popover so its filter operates on fresh
        // data immediately. Constructed lazily in `component.rs`,
        // so during init this is `None` and the popover picks up
        // the list when it first opens.
        if let Some(pop) = &self.mention_popover {
            pop.set_candidates(candidates);
        }

        // Re-resolve mention chips on every already-rendered row.
        // Without this, rows that landed in the list before the
        // candidate event would forever display `@<digits>` while
        // newly-appended rows showed `@Name`.
        let inv = self.mentions.clone();
        self.update_items_where(
            |_| true,
            move |it| {
                it.resolve_mentions(|d| inv.name_for_digits(d));
            },
        );
    }
}
