// Media-state handlers: ready / failed / user-initiated download.

use std::collections::HashSet;

use relm4::ComponentSender;

use super::super::messages::ChatTabOutput;
use super::super::model::ChatTab;

/// Ordered ranks for `delivery_status`. Used to gate row repaint
/// requests: only flip the icon when the new status is strictly
/// higher in the chain (peers occasionally deliver a `delivered`
/// receipt after the user already pressed read).
fn status_rank(s: &str) -> u8 {
    match s {
        "pending" => 0,
        "sent" => 1,
        "server_ack" => 1,
        "delivered" => 2,
        "read" => 3,
        "played" => 4,
        "failed" => 5,
        _ => 0,
    }
}

impl ChatTab {
    pub(in crate::components::chat_tab) fn handle_media_ready(
        &mut self,
        message_ids: Vec<String>,
        path: String,
        mimetype: Option<String>,
    ) {
        // Drop the cached texture for any path being rotated out so a
        // The visual_media widget always decodes through glycin
        // when the path changes (see `spawn_glycin_load`); the
        // load_gen check there guarantees the previous texture is
        // discarded the moment the row is rebound, so no manual
        // cache invalidation is needed.
        let id_set: HashSet<String> = message_ids.iter().cloned().collect();
        self.update_items_where(
            |it| id_set.contains(&it.id),
            |it| {
                it.media_path = Some(path.clone());
                it.media_status = "done".to_string();
                if it.media_mimetype.is_none() {
                    it.media_mimetype = mimetype.clone();
                }
            },
        );
    }

    pub(in crate::components::chat_tab) fn handle_media_failed(&mut self, message_id: String) {
        self.update_items_where(
            |it| it.id == message_id,
            |it| {
                it.media_path = None;
                it.media_status = "failed".to_string();
            },
        );
    }

    pub(in crate::components::chat_tab) fn handle_receipt_update(
        &mut self,
        message_ids: Vec<String>,
        status: String,
    ) {
        let id_set: HashSet<&String> = message_ids.iter().collect();
        self.update_items_where(
            |it| {
                if !it.from_me {
                    return false;
                }
                if !id_set.contains(&it.id) {
                    return false;
                }
                // Don't downgrade: read > delivered > sent > pending.
                // The wire status arrives out-of-order sometimes (a
                // delivered receipt after a read one for the same
                // message group).
                let cur = status_rank(&it.delivery_status);
                let new = status_rank(&status);
                new > cur
            },
            |it| {
                it.delivery_status = status.clone();
            },
        );
    }

    pub(in crate::components::chat_tab) fn handle_request_media_download(
        &mut self,
        id: String,
        sender: &ComponentSender<Self>,
    ) {
        // Mark the in-flight state in the shared inventory so
        // closing/reopening the tab doesn't lose the spinner.
        self.media.set_downloading(&id);
        let target_id = id.clone();
        self.update_items_where(
            |it| it.id == target_id,
            |it| {
                it.media_path = None;
                it.media_status = "downloading".to_string();
            },
        );
        let _ = sender.output(ChatTabOutput::RequestMediaDownload(id));
    }
}
