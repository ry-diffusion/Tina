// Renders a QR string into a `gdk::Texture` so it can be plugged into a
// `gtk::Picture`. We render at integer scale + quiet-zone margin (the
// WhatsApp scanner is fussy about both) and let GTK do final scaling via
// the Picture.

use gdk::prelude::*;
use gtk::gdk;

pub fn render_qr_texture(qr: &str) -> Option<gdk::Texture> {
    let code = qrcode::QrCode::new(qr).ok()?;
    let scale: usize = 8;
    let margin: usize = 4;
    let width = code.width();
    let size = (width + margin * 2) * scale;
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
