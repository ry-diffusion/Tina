// Discord/WhatsApp-style media overlay: an `adw::Dialog` that presents
// over the app's main window with the media filling the body, a HeaderBar
// carrying Save / Open-externally actions, and Escape / close button to
// dismiss.
//
// `anchor` is any widget inside the parent window — `adw::Dialog` walks
// the tree up to find the AdwApplicationWindow it should attach to. We
// use the clicked widget itself.

use adw::prelude::*;
use gtk::gio;

pub fn open_media_lightbox(anchor: &impl IsA<gtk::Widget>, path: String, message_type: String) {
    let dialog = adw::Dialog::builder()
        .content_width(1100)
        .content_height(800)
        .build();

    let header = adw::HeaderBar::new();
    header.set_show_end_title_buttons(true);
    header.set_title_widget(Some(&adw::WindowTitle::new(
        match message_type.as_str() {
            "video" => "Video",
            "sticker" => "Sticker",
            _ => "Image",
        },
        &short_filename(&path),
    )));

    pack_open_externally(&header, &path);
    pack_save_as(&header, &path, anchor);

    let body: gtk::Widget = match message_type.as_str() {
        "video" => {
            let video = gtk::Video::for_filename(Some(std::path::Path::new(&path)));
            video.set_autoplay(true);
            video.set_hexpand(true);
            video.set_vexpand(true);
            video.upcast()
        }
        _ => {
            let pic = gtk::Picture::for_filename(&path);
            pic.set_can_shrink(true);
            pic.set_content_fit(gtk::ContentFit::Contain);
            pic.set_hexpand(true);
            pic.set_vexpand(true);
            pic.upcast()
        }
    };

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&body));

    dialog.set_child(Some(&toolbar));
    dialog.present(Some(anchor.upcast_ref::<gtk::Widget>()));
}

/// "Open externally" — hands the file to the system handler. Useful for
/// video editors / image viewers / external default apps.
fn pack_open_externally(header: &adw::HeaderBar, path: &str) {
    let btn = gtk::Button::from_icon_name("document-open-symbolic");
    btn.set_tooltip_text(Some("Open externally"));
    btn.add_css_class("flat");
    let p = path.to_string();
    btn.connect_clicked(move |_| {
        let file = gio::File::for_path(&p);
        let launcher = gtk::FileLauncher::new(Some(&file));
        launcher.launch(gtk::Window::NONE, gio::Cancellable::NONE, |_| {});
    });
    header.pack_end(&btn);
}

/// "Save as…" — copies the cached file into a user-chosen location.
fn pack_save_as(header: &adw::HeaderBar, path: &str, anchor: &impl IsA<gtk::Widget>) {
    let btn = gtk::Button::from_icon_name("document-save-symbolic");
    btn.set_tooltip_text(Some("Save as…"));
    btn.add_css_class("flat");
    let p = path.to_string();
    let anchor_weak = anchor.upcast_ref::<gtk::Widget>().downgrade();
    btn.connect_clicked(move |_| {
        let save = gtk::FileDialog::builder()
            .title("Save media")
            .initial_name(default_save_name(&p))
            .modal(true)
            .build();
        let parent_window = anchor_weak
            .upgrade()
            .and_then(|w| w.root())
            .and_then(|r| r.downcast::<gtk::Window>().ok());
        let src = std::path::PathBuf::from(&p);
        save.save(parent_window.as_ref(), gio::Cancellable::NONE, move |res| {
            let Ok(file) = res else { return };
            let Some(dest) = file.path() else { return };
            if let Err(e) = std::fs::copy(&src, &dest) {
                tracing::warn!(?dest, error = %e, "lightbox: save copy failed");
            }
        });
    });
    header.pack_end(&btn);
}

fn short_filename(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn default_save_name(path: &str) -> String {
    let base = short_filename(path);
    if base.is_empty() {
        "media".to_string()
    } else {
        base
    }
}
