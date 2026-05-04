// Image decoding helper.
//
// `Picture::set_filename` bypasses GdkPixbuf for known formats and has
// historically failed silently on WebP stickers; this helper avoids that
// by going through GdkPixbuf unconditionally — which honours
// `webp-pixbuf-loader` and any other registered loaders.

pub fn load_image_paintable(path: Option<&str>) -> Option<gtk::gdk::Texture> {
    let path = path?;
    if path.is_empty() {
        return None;
    }
    match gtk::gdk_pixbuf::Pixbuf::from_file(path) {
        Ok(pixbuf) => {
            // `Texture::for_pixbuf` is deprecated since GDK 4.20 but its
            // replacement (`gdk-pixbuf` → `gdk::MemoryTexture` round-trip)
            // is materially slower and skips the loader-specific decoding
            // we rely on for WebP. Keep the deprecated path until GTK
            // exposes a direct equivalent.
            #[allow(deprecated)]
            Some(gtk::gdk::Texture::for_pixbuf(&pixbuf))
        }
        Err(e) => {
            tracing::warn!(path, error = %e, "failed to decode image via GdkPixbuf");
            None
        }
    }
}
