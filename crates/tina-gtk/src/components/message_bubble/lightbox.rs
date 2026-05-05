// Discord/WhatsApp-style media overlay: an `adw::Dialog` that presents
// over the app's main window with the media filling the body, a HeaderBar
// carrying Save / Open-externally actions, and Escape / close button to
// dismiss.
//
// `anchor` is any widget inside the parent window — `adw::Dialog` walks
// the tree up to find the AdwApplicationWindow it should attach to. We
// use the clicked widget itself.

use adw::prelude::*;
use crate::fl;
use glib::clone;
use gtk::gio;

pub fn open_media_lightbox(anchor: &impl IsA<gtk::Widget>, path: String, message_type: String) {
    let dialog = adw::Dialog::builder()
        .content_width(1100)
        .content_height(800)
        .build();

    let header = adw::HeaderBar::new();
    header.set_show_end_title_buttons(true);
    header.set_title_widget(Some(&adw::WindowTitle::new(
        &match message_type.as_str() {
            "video" => fl!("lightbox-video"),
            "sticker" => fl!("lightbox-sticker"),
            _ => fl!("lightbox-image"),
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
            // Decode through glycin so the lightbox honours the same
            // sandboxed pipeline as inline bubbles. Without this the
            // attack surface for things like CVE-2023-4863 (libwebp
            // RCE) opens up the moment the user taps a malicious
            // image. Loading is async — we install the paintable as
            // soon as the first frame lands; until then a spinner
            // sits on the body.
            let stack = gtk::Stack::builder()
                .transition_type(gtk::StackTransitionType::Crossfade)
                .build();
            let spinner = gtk::Spinner::new();
            spinner.set_spinning(true);
            spinner.set_halign(gtk::Align::Center);
            spinner.set_valign(gtk::Align::Center);
            spinner.set_width_request(48);
            spinner.set_height_request(48);
            stack.add_named(&spinner, Some("loading"));

            let pic = gtk::Picture::new();
            pic.set_can_shrink(true);
            pic.set_content_fit(gtk::ContentFit::Contain);
            pic.set_hexpand(true);
            pic.set_vexpand(true);
            stack.add_named(&pic, Some("media"));
            stack.set_visible_child_name("loading");

            let load_path = path.clone();
            glib::MainContext::default().spawn_local(clone!(
                #[weak]
                stack,
                #[weak]
                pic,
                async move {
                    let file = gio::File::for_path(&load_path);
                    let loader = glycin::Loader::new(file);
                    let result: Option<(glycin::Image, glycin::Frame)> = async {
                        let image = loader.load().await.ok()?;
                        let first = image.next_frame().await.ok()?;
                        Some((image, first))
                    }
                    .await;
                    let Some((image, first)) = result else {
                        tracing::debug!(
                            path = %load_path,
                            "lightbox glycin load failed",
                        );
                        return;
                    };
                    let paintable =
                        crate::components::message_media::AnimatedImagePaintable::new(
                            image, first, true,
                        );
                    pic.set_paintable(Some(&paintable));
                    stack.set_visible_child_name("media");
                }
            ));

            stack.set_hexpand(true);
            stack.set_vexpand(true);
            stack.upcast()
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
    btn.set_tooltip_text(Some(&fl!("lightbox-open-externally")));
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
    btn.set_tooltip_text(Some(&fl!("lightbox-save-as")));
    btn.add_css_class("flat");
    let p = path.to_string();
    let anchor_weak = anchor.upcast_ref::<gtk::Widget>().downgrade();
    btn.connect_clicked(move |_| {
        let save = gtk::FileDialog::builder()
            .title(&fl!("lightbox-save-media"))
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
