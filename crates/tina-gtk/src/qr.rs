// Renders a QR string into a `gdk::Texture` so it can be plugged into a
// `gtk::Picture`. We render at integer scale + quiet-zone margin (the
// WhatsApp scanner is fussy about both) and target a *capped* output
// size so the Picture's natural dimensions stay below the parent
// Stack's request — otherwise the Stack picks the texture's natural
// size as its allocation and the card grows past the loading state.

use gdk::prelude::*;
use gtk::gdk;

/// Maximum texture edge in pixels. Picked to match the QR Stack's
/// `set_*_request` in `components/login.rs` so the rendered Picture's
/// natural size is never larger than the loading state's footprint.
const MAX_TEXTURE_SIZE: usize = 220;

pub fn render_qr_texture(qr: &str) -> Option<gdk::Texture> {
    let code = qrcode::QrCode::new(qr).ok()?;
    let margin: usize = 4;
    let width = code.width();
    let total_modules = width + margin * 2;
    if total_modules == 0 {
        return None;
    }
    // Largest integer scale that still fits inside the cap. WhatsApp's
    // QRs are typically 33-49 modules wide; with `margin = 4` and
    // `MAX_TEXTURE_SIZE = 220` this lands at scale 4–5, plenty for the
    // phone scanner to read.
    let scale = (MAX_TEXTURE_SIZE / total_modules).max(1);
    let size = total_modules * scale;
    if size == 0 {
        return None;
    }

    // RGBA8 buffer, white background, dark modules in black.
    let mut buf: Vec<u8> = vec![0xFF; size * size * 4];
    let stride = size * 4;
    for y in 0..width {
        for x in 0..width {
            if code[(x, y)] != qrcode::types::Color::Dark {
                continue;
            }
            for dy in 0..scale {
                for dx in 0..scale {
                    let px = (x + margin) * scale + dx;
                    let py = (y + margin) * scale + dy;
                    let off = py * stride + px * 4;
                    buf[off] = 0;
                    buf[off + 1] = 0;
                    buf[off + 2] = 0;
                    buf[off + 3] = 0xFF;
                }
            }
        }
    }

    let bytes = glib::Bytes::from_owned(buf);
    Some(gdk::MemoryTexture::new(
        size as i32,
        size as i32,
        gdk::MemoryFormat::R8g8b8a8,
        &bytes,
        stride,
    )
    .upcast())
}
