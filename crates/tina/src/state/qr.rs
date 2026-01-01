use qrcode::{QrCode, types::Color};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

pub(crate) fn render_qr_image(qr: &str) -> Option<Image> {
    let code = QrCode::new(qr).ok()?;
    let scale: usize = 8;
    let margin: usize = 4;
    let width = code.width();
    let size = (width + margin * 2) * scale;

    if size == 0 {
        return None;
    }

    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(size as u32, size as u32);

    {
        let slice = buffer.make_mut_slice();
        for pixel in slice.iter_mut() {
            *pixel = Rgba8Pixel {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            };
        }

        for y in 0..width {
            for x in 0..width {
                if code[(x, y)] != Color::Dark {
                    continue;
                }
                for dy in 0..scale {
                    for dx in 0..scale {
                        let px = (x + margin) * scale + dx;
                        let py = (y + margin) * scale + dy;
                        let idx = py * size + px;
                        if let Some(pixel) = slice.get_mut(idx) {
                            *pixel = Rgba8Pixel {
                                r: 0,
                                g: 0,
                                b: 0,
                                a: 255,
                            };
                        }
                    }
                }
            }
        }
    }

    Some(Image::from_rgba8(buffer))
}
