// Media-state handlers: ready / failed / user-initiated download.

use std::collections::HashSet;

use relm4::ComponentSender;

use crate::components::message_bubble::MessageBubbleInput;

use super::super::messages::ChatTabOutput;
use super::super::model::ChatTab;

impl ChatTab {
    pub(in crate::components::chat_tab) fn handle_media_ready(
        &mut self,
        message_ids: Vec<String>,
        path: String,
        mimetype: Option<String>,
    ) {
        // Mutate the factory items in place via per-row Input — no
        // remove+insert, so the listbox keeps the same widget hierarchy
        // and the scroll position never jumps.
        let id_set: HashSet<&String> = message_ids.iter().collect();
        let indices: Vec<usize> = self
            .messages
            .guard()
            .iter()
            .enumerate()
            .filter_map(|(i, f)| if id_set.contains(&f.item.id) { Some(i) } else { None })
            .collect();
        for idx in indices {
            self.messages.send(
                idx,
                MessageBubbleInput::UpdateMedia {
                    path: Some(path.clone()),
                    status: "done".into(),
                    mimetype: mimetype.clone(),
                },
            );
        }
    }

    pub(in crate::components::chat_tab) fn handle_media_failed(&mut self, message_id: String) {
        let indices: Vec<usize> = self
            .messages
            .guard()
            .iter()
            .enumerate()
            .filter_map(|(i, f)| if f.item.id == message_id { Some(i) } else { None })
            .collect();
        for idx in indices {
            self.messages.send(
                idx,
                MessageBubbleInput::UpdateMedia {
                    path: None,
                    status: "failed".into(),
                    mimetype: None,
                },
            );
        }
    }

    pub(in crate::components::chat_tab) fn handle_request_media_download(
        &mut self,
        id: String,
        sender: &ComponentSender<Self>,
    ) {
        // Mark the in-flight state in the shared inventory so
        // closing/reopening the tab doesn't lose the spinner.
        self.media.set_downloading(&id);
        // In-place factory update via per-row Input — the listbox keeps
        // the same widget instance, so no row-rebuild and no scroll
        // jump on click.
        let indices: Vec<usize> = self
            .messages
            .guard()
            .iter()
            .enumerate()
            .filter_map(|(i, f)| if f.item.id == id { Some(i) } else { None })
            .collect();
        for idx in indices {
            self.messages.send(
                idx,
                MessageBubbleInput::UpdateMedia {
                    path: None,
                    status: "downloading".into(),
                    mimetype: None,
                },
            );
        }
        let _ = sender.output(ChatTabOutput::RequestMediaDownload(id));
    }
}
