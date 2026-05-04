// Sticker-picker handlers: open the popover (kicking off a worker
// fetch), repaint when results arrive, and send a chosen sticker
// straight without going through the preview dialog (matches the
// WhatsApp mobile UX where stickers are a one-tap send).

use gtk::prelude::*;
use relm4::ComponentSender;

use super::super::messages::{ChatTabInput, ChatTabOutput};
use super::super::model::ChatTab;

const STICKER_TILE_PX: i32 = 84;

impl ChatTab {
    pub(in crate::components::chat_tab) fn handle_open_sticker_picker(
        &mut self,
        sender: &ComponentSender<Self>,
    ) {
        // Ask for the catalog every time — the user may have
        // received new stickers since the last open.
        let _ = sender.output(ChatTabOutput::RequestStickers {
            chat_id: self.chat_id.clone(),
        });
        if let Some(pop) = &self.sticker_popover {
            pop.popup();
        }
    }

    pub(in crate::components::chat_tab) fn handle_stickers_loaded(
        &mut self,
        items: Vec<(String, String)>,
        sender: &ComponentSender<Self>,
    ) {
        let Some(grid) = self.sticker_grid.as_ref() else {
            return;
        };
        // Drain old tiles before repainting. FlowBox.remove_all is
        // GTK 4.12+; iterating + remove keeps us on the safe side
        // of the version dance.
        while let Some(child) = grid.first_child() {
            grid.remove(&child);
        }
        if items.is_empty() {
            let empty = gtk::Label::builder()
                .label("No stickers yet — receive one to add it here.")
                .css_classes(["dim-label"])
                .margin_top(12)
                .margin_bottom(12)
                .margin_start(12)
                .margin_end(12)
                .build();
            grid.append(&empty);
            return;
        }
        for (path, _mimetype) in items {
            let tile = build_sticker_tile(&path, sender.clone());
            grid.append(&tile);
        }
    }

    pub(in crate::components::chat_tab) fn handle_send_sticker_path(
        &mut self,
        path: String,
        sender: &ComponentSender<Self>,
    ) {
        if let Some(pop) = &self.sticker_popover {
            pop.popdown();
        }
        // Bypass the preview dialog: stickers don't take captions
        // and the picker already gave the user a visual preview.
        self.handle_send_media(
            tina_core::MediaKind::Sticker,
            path,
            None,
            Some("image/webp".into()),
            None,
            sender,
        );
    }
}

fn build_sticker_tile(path: &str, sender: ComponentSender<ChatTab>) -> gtk::Widget {
    // gtk::Picture decodes lazily, so even a 64-tile grid stays
    // responsive on slow disks. Falls back to a generic icon if the
    // file is unreadable (deleted from cache, broken symlink).
    let pic: gtk::Widget = match gdk::Texture::from_filename(path) {
        Ok(tex) => {
            let p = gtk::Picture::for_paintable(&tex);
            p.set_can_shrink(true);
            p.set_content_fit(gtk::ContentFit::Contain);
            p.set_size_request(STICKER_TILE_PX, STICKER_TILE_PX);
            p.upcast()
        }
        Err(_) => {
            let icon = gtk::Image::from_icon_name("image-missing-symbolic");
            icon.set_pixel_size(STICKER_TILE_PX / 2);
            icon.upcast()
        }
    };
    let btn = gtk::Button::builder()
        .css_classes(["flat"])
        .child(&pic)
        .build();
    let path_owned = path.to_string();
    btn.connect_clicked(move |_| {
        let _ = sender
            .input_sender()
            .send(ChatTabInput::SendStickerByPath(path_owned.clone()));
    });
    btn.upcast()
}
