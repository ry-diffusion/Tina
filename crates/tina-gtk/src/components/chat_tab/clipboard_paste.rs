// Ctrl+V image paste for the composer entry.
//
// `gtk::Entry`'s built-in paste only consumes text formats. To support
// pasting screenshots / images copied from a browser we install a
// capture-phase key controller that fires BEFORE the entry's default
// handler: when the clipboard exposes a `GdkTexture`, we save it to a
// tmp file, route it through `ChatTabInput::AttachFile` (same path as
// the file-picker), and stop the propagation so the entry doesn't end
// up pasting the texture's debug-string representation.

use gtk::gdk;
use gtk::gio;
use gtk::prelude::*;
use gtk::{glib, glib::clone};
use relm4::Sender;

use super::messages::ChatTabInput;

pub fn wire_paste(entry: &gtk::Entry, input: Sender<ChatTabInput>) {
    let key_ctl = gtk::EventControllerKey::new();
    key_ctl.set_propagation_phase(gtk::PropagationPhase::Capture);
    key_ctl.connect_key_pressed(clone!(
        #[weak] entry,
        #[strong] input,
        #[upgrade_or] glib::Propagation::Proceed,
        move |_ctl, keyval, _kc, mods| {
            if !mods.contains(gdk::ModifierType::CONTROL_MASK) {
                return glib::Propagation::Proceed;
            }
            if keyval != gdk::Key::v && keyval != gdk::Key::V {
                return glib::Propagation::Proceed;
            }
            let clipboard = entry.clipboard();
            if !clipboard_has_image(&clipboard) {
                return glib::Propagation::Proceed;
            }
            let input = input.clone();
            clipboard.read_texture_async(
                None::<&gio::Cancellable>,
                move |result| {
                    let Ok(Some(texture)) = result else { return };
                    if let Some((path, kind, mimetype)) = stash_texture(&texture) {
                        let _ = input.send(ChatTabInput::AttachFile {
                            kind,
                            path,
                            mimetype: Some(mimetype),
                            filename: None,
                        });
                    }
                },
            );
            glib::Propagation::Stop
        }
    ));
    entry.add_controller(key_ctl);
}

/// Probe the clipboard's advertised mime types. `read_texture_async`
/// would itself fail gracefully on a text-only clipboard but we
/// intercept Ctrl+V at capture phase, so checking up-front lets the
/// default text-paste path run when there's no image.
fn clipboard_has_image(clipboard: &gdk::Clipboard) -> bool {
    let formats = clipboard.formats();
    for mime in [
        "image/png",
        "image/jpeg",
        "image/webp",
        "image/bmp",
        "image/gif",
    ] {
        if formats.contain_mime_type(mime) {
            return true;
        }
    }
    false
}

/// Persist a `GdkTexture` to a tmp PNG and return the path so the
/// rest of the attach pipeline can treat it like a file the user
/// picked. PNG keeps the alpha channel screenshots usually carry.
fn stash_texture(texture: &gdk::Texture) -> Option<(String, tina_core::MediaKind, String)> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let path = std::env::temp_dir()
        .join(format!("tina-paste-{nanos}.png"))
        .to_string_lossy()
        .to_string();
    let bytes = texture.save_to_png_bytes();
    if bytes.is_empty() {
        return None;
    }
    if std::fs::write(&path, bytes.as_ref()).is_err() {
        return None;
    }
    Some((path, tina_core::MediaKind::Image, "image/png".to_string()))
}
