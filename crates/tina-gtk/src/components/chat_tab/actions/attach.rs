// Attach / record / send-media action handlers, mirrors history.rs's
// shape for text Send. Three stages:
//
//   1. PickAttachment(kind) — opens a file dialog filtered to the kind,
//      forwards the chosen path back as AttachFile.
//   2. AttachFile         — opens the preview dialog, which on accept
//      fires SendMedia.
//   3. SendMedia           — synthesises an optimistic echo and
//      forwards `ChatTabOutput::SendMedia` to the parent.
//
// Audio recording is its own flow (ToggleRecord / RecordingFinished /
// RecordingFailed) but shares the SendMedia tail.

use std::io::Read;
use std::path::Path;

use gtk::gio;
use gtk::prelude::*;
use relm4::ComponentSender;
use sha2::{Digest, Sha256};

use crate::components::message_bubble::MessageItem;

use super::super::messages::{ChatTabInput, ChatTabOutput, COLLAPSE_WINDOW_SECS};
use super::super::model::ChatTab;
use super::super::preview;
use super::super::record;

impl ChatTab {
    pub(in crate::components::chat_tab) fn handle_pick_attachment(
        &mut self,
        kind: tina_core::MediaKind,
        sender: &ComponentSender<Self>,
    ) {
        let Some(window) = self
            .scroll
            .as_ref()
            .and_then(|s| s.root())
            .and_downcast::<gtk::Window>()
        else {
            return;
        };
        let dialog = gtk::FileDialog::new();
        dialog.set_title(file_dialog_title(kind));
        dialog.set_modal(true);
        if let Some(filter) = file_filter_for(kind) {
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            dialog.set_filters(Some(&filters));
            dialog.set_default_filter(Some(&filter));
        }
        let sender = sender.clone();
        dialog.open(
            Some(&window),
            None::<&gio::Cancellable>,
            move |result| {
                let Ok(file) = result else { return };
                let Some(path) = file.path() else { return };
                let path_str = path.to_string_lossy().to_string();
                let filename = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string());
                let mimetype = guess_mimetype(&path_str);
                let _ = sender.input_sender().send(ChatTabInput::AttachFile {
                    kind,
                    path: path_str,
                    mimetype,
                    filename,
                });
            },
        );
    }

    pub(in crate::components::chat_tab) fn handle_attach_file(
        &mut self,
        kind: tina_core::MediaKind,
        path: String,
        mimetype: Option<String>,
        filename: Option<String>,
        sender: &ComponentSender<Self>,
    ) {
        // Dialog needs a parent widget to attach to; fall back to
        // bailing out if the scrolled-window's root chain isn't a
        // Window (shouldn't happen in practice, but keeps the path
        // panic-free).
        let Some(parent) = self
            .scroll
            .as_ref()
            .and_then(|s| s.root())
            .and_downcast::<gtk::Window>()
        else {
            return;
        };
        preview::present(parent.upcast_ref(), sender.clone(), kind, path, mimetype, filename);
    }

    pub(in crate::components::chat_tab) fn handle_send_media(
        &mut self,
        kind: tina_core::MediaKind,
        path: String,
        caption: Option<String>,
        mimetype: Option<String>,
        filename: Option<String>,
        sender: &ComponentSender<Self>,
    ) {
        // Compute SHA-256 of the source file once — both the
        // optimistic echo and the matching server-echo carry it on
        // `media_sha256`, so the dedup is exact (no body-text hack
        // that breaks for empty captions).
        let sha256_hex = sha256_of_file(&path);

        let mut local_item = self.build_optimistic_media_echo(
            kind,
            &path,
            caption.as_deref(),
            mimetype.as_deref(),
            filename.as_deref(),
        );
        local_item.media_sha256 = sha256_hex.clone();
        let local_id = local_item.id.clone();
        self.seen_message_ids.insert(local_id.clone());
        if let Some(sha) = sha256_hex.clone() {
            self.pending_media_echoes
                .entry(sha)
                .or_default()
                .push_back(local_id);
        } else {
            // SHA-256 failed (file unreadable / oversized) — fall
            // back to the body-text key. The dispatcher's belt-and-
            // suspenders refetch papers over the rest.
            self.pending_echoes
                .entry(media_echo_key(&path, caption.as_deref()))
                .or_default()
                .push_back(local_id);
        }
        self.bottomed.set(true);
        {
            let mut guard = self.messages.guard();
            guard.push_back(local_item);
        }
        let _ = sender.output(ChatTabOutput::SendMedia {
            chat_id: self.chat_id.clone(),
            kind,
            path,
            caption,
            mimetype,
            filename,
        });
    }

    pub(in crate::components::chat_tab) fn handle_toggle_record(
        &mut self,
        sender: &ComponentSender<Self>,
    ) {
        if let Some(handle) = self.recorder.take() {
            // Stopping. Run on a background thread so we don't block
            // the GTK loop while gst-launch flushes — but we need to
            // hop back via glib::idle_add_local to feed the result
            // into the input bus.
            self.recording_active.set(false);
            let sender = sender.clone();
            std::thread::spawn(move || {
                match record::stop(handle) {
                    Ok((path, seconds)) => {
                        let _ = sender
                            .input_sender()
                            .send(ChatTabInput::RecordingFinished { path, seconds });
                    }
                    Err(e) => {
                        let _ = sender
                            .input_sender()
                            .send(ChatTabInput::RecordingFailed(e));
                    }
                }
            });
        } else {
            match record::start() {
                Ok(handle) => {
                    self.recording_active.set(true);
                    self.recorder = Some(handle);
                }
                Err(e) => {
                    let _ = sender
                        .input_sender()
                        .send(ChatTabInput::RecordingFailed(e));
                }
            }
        }
    }

    pub(in crate::components::chat_tab) fn handle_recording_finished(
        &mut self,
        path: String,
        _seconds: u32,
        sender: &ComponentSender<Self>,
    ) {
        // Voice notes go straight through the preview dialog — the
        // user gets a chance to listen before sending. The dialog
        // skips the caption box for `Voice`.
        self.handle_attach_file(
            tina_core::MediaKind::Voice,
            path,
            Some("audio/ogg; codecs=opus".to_string()),
            None,
            sender,
        );
    }

    pub(in crate::components::chat_tab) fn handle_recording_failed(&mut self, error: String) {
        // The toast goes up via the parent window's AdwToastOverlay;
        // we don't have a direct handle here, so log + ignore. The UI
        // could route this via ChatTabOutput in a follow-up; keeping
        // the crate-level surface minimal for now.
        tracing::error!("voice record failed: {error}");
        self.recording_active.set(false);
        self.recorder = None;
    }

    /// Build the local-only `MessageItem` for an optimistic media echo.
    /// Mirrors `build_optimistic_echo` (text path) but with the media
    /// fields pre-filled to point at the source path on disk so the
    /// bubble renders the picture/preview without a download.
    fn build_optimistic_media_echo(
        &self,
        kind: tina_core::MediaKind,
        path: &str,
        caption: Option<&str>,
        mimetype: Option<&str>,
        filename: Option<&str>,
    ) -> MessageItem {
        let local_id = format!(
            "local-media-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        );
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or_default();
        let (cursor_sender, cursor_ts) = self.factory_tail_cursor();
        let local_collapsed = match (cursor_sender.as_deref(), cursor_ts) {
            (Some("\0me"), Some(prev_ts)) => {
                now_unix.saturating_sub(prev_ts) <= COLLAPSE_WINDOW_SECS
            }
            _ => false,
        };
        let local_avatar = self
            .user_jid
            .as_ref()
            .and_then(|j| self.avatars.get(j.raw()));
        let summary = caption
            .map(|s| s.to_string())
            .unwrap_or_else(|| match kind {
                tina_core::MediaKind::Image => "[Image]".into(),
                tina_core::MediaKind::Video => "[Video]".into(),
                tina_core::MediaKind::Audio | tina_core::MediaKind::Voice => "[Audio]".into(),
                tina_core::MediaKind::Sticker => "[Sticker]".into(),
                tina_core::MediaKind::Document => "[Document]".into(),
            });
        let size_bytes = std::fs::metadata(path).ok().map(|m| m.len() as i64);
        let mt_string = mimetype.map(|s| s.to_string()).or_else(|| guess_mimetype(path));
        MessageItem {
            id: local_id,
            from_me: true,
            sender_name: String::new(),
            sender_jid: self.user_jid.as_ref().map(|x| x.raw().to_string()),
            sender_avatar_path: local_avatar,
            chat_kind: self.kind.clone(),
            chat_display_name: if self.name.is_empty() {
                None
            } else {
                Some(self.name.clone())
            },
            chat_avatar_path: self.avatars.get(&self.chat_id),
            is_collapsed: local_collapsed,
            content: summary.clone(),
            message_type: match kind {
                tina_core::MediaKind::Voice => "audio".to_string(),
                k => k.as_str().to_string(),
            },
            timestamp: crate::time::format_message_time(now_unix),
            timestamp_unix: now_unix,
            media_summary: summary,
            media_mimetype: mt_string,
            media_size_bytes: size_bytes,
            media_duration_secs: None,
            media_path: Some(path.to_string()),
            media_status: "done".to_string(),
            media_filename: filename.map(|s| s.to_string()),
            media_sha256: None,
            delivery_status: "pending".to_string(),
            thumbnail: None,
            quoted_message_id: None,
            quoted_sender_id: None,
            quoted_sender_name: None,
            quoted_preview: None,
            mentions: Vec::new(),
        }
    }
}

fn file_dialog_title(kind: tina_core::MediaKind) -> &'static str {
    match kind {
        tina_core::MediaKind::Image => "Send a photo",
        tina_core::MediaKind::Video => "Send a video",
        tina_core::MediaKind::Audio => "Send audio",
        tina_core::MediaKind::Voice => "Send a voice note",
        tina_core::MediaKind::Sticker => "Send a sticker",
        tina_core::MediaKind::Document => "Send a document",
    }
}

fn file_filter_for(kind: tina_core::MediaKind) -> Option<gtk::FileFilter> {
    let filter = gtk::FileFilter::new();
    match kind {
        tina_core::MediaKind::Image => {
            filter.set_name(Some("Images"));
            filter.add_mime_type("image/jpeg");
            filter.add_mime_type("image/png");
            filter.add_mime_type("image/webp");
            filter.add_mime_type("image/gif");
        }
        tina_core::MediaKind::Video => {
            filter.set_name(Some("Videos"));
            filter.add_mime_type("video/mp4");
            filter.add_mime_type("video/webm");
            filter.add_mime_type("video/quicktime");
        }
        tina_core::MediaKind::Audio | tina_core::MediaKind::Voice => {
            filter.set_name(Some("Audio"));
            filter.add_mime_type("audio/ogg");
            filter.add_mime_type("audio/mpeg");
            filter.add_mime_type("audio/mp4");
            filter.add_mime_type("audio/aac");
        }
        tina_core::MediaKind::Sticker => {
            filter.set_name(Some("Stickers (.webp)"));
            filter.add_mime_type("image/webp");
            filter.add_pattern("*.webp");
        }
        tina_core::MediaKind::Document => {
            filter.set_name(Some("All files"));
            filter.add_pattern("*");
        }
    }
    Some(filter)
}

fn guess_mimetype(path: &str) -> Option<String> {
    let p = Path::new(path);
    let ext = p.extension()?.to_str()?.to_ascii_lowercase();
    Some(
        match ext.as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "webp" => "image/webp",
            "gif" => "image/gif",
            "mp4" => "video/mp4",
            "webm" => "video/webm",
            "mov" => "video/quicktime",
            "ogg" | "oga" | "opus" => "audio/ogg",
            "mp3" => "audio/mpeg",
            "m4a" | "aac" => "audio/mp4",
            "pdf" => "application/pdf",
            _ => return None,
        }
        .to_string(),
    )
}

fn sha256_of_file(path: &str) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(hex::encode(hasher.finalize()))
}

fn media_echo_key(path: &str, caption: Option<&str>) -> String {
    // Best-effort match key. The text-side echo deduplicates on
    // body text; for media we don't yet know the sha256 the worker
    // computes, so we fall back to the source path + caption. The
    // belt-and-suspenders re-fetch in `service::send_media` covers
    // the case where the key drifts (worker re-encodes, normalises,
    // etc) — the dup row will be filtered by `seen_message_ids`
    // when the real one arrives.
    match caption {
        Some(c) => format!("{path}\n{c}"),
        None => path.to_string(),
    }
}
