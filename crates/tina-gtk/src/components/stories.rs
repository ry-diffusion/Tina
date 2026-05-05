// Stories viewer for the Status tab. Presentation pattern mirrors
// `lightbox.rs` (adw::Dialog + AdwToolbarView + dialog.present(anchor)),
// but the body is an `adw::Carousel` carrying every status post the
// author has up — same UX as WhatsApp / Instagram stories: tap left
// half = previous, tap right half = next, swipe nativo do AdwCarousel,
// auto-advance after 5 s per post, segmented progress bars at the top
// showing the timeline.

use std::cell::Cell;
use crate::fl;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use tina_db::MessageRow;


/// Per-post auto-advance dwell time. WhatsApp uses ~5 s for images
/// and the duration of the clip for video; we treat both the same
/// for simplicity — videos shorter than 5 s loop, longer ones get
/// cut off, but the user can tap the right half to advance early.
const AUTO_ADVANCE_MS: u64 = 5_000;
/// Progress-bar tick interval. 50 ms gives 100 frames over 5 s,
/// smooth enough that the bar fill looks animated without burning
/// frames during off-screen ticks.
const TICK_MS: u64 = 50;

pub fn open_stories_viewer<W: IsA<gtk::Widget>>(
    parent: &W,
    author_name: &str,
    posts: Vec<MessageRow>,
) {
    if posts.is_empty() {
        return;
    }

    let dialog = adw::Dialog::builder()
        .content_width(540)
        .content_height(820)
        .title(author_name)
        .build();

    let header = adw::HeaderBar::new();
    header.set_show_end_title_buttons(true);
    header.set_title_widget(Some(&adw::WindowTitle::new(author_name, &fl!("stories-status-subtitle"))));

    // Progress bar strip — one per post, stacked horizontally.
    // `osd` styles them as the white slivers WhatsApp uses; the
    // active bar fills via the timer below, completed bars stay at
    // 1.0, upcoming bars stay at 0.0.
    let progress_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(4)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(12)
        .margin_end(12)
        .build();
    let mut bars: Vec<gtk::ProgressBar> = Vec::with_capacity(posts.len());
    for _ in 0..posts.len() {
        let bar = gtk::ProgressBar::builder().hexpand(true).build();
        bar.add_css_class("osd");
        progress_box.append(&bar);
        bars.push(bar);
    }

    // Carousel: one page per post.
    let carousel = adw::Carousel::builder()
        .vexpand(true)
        .hexpand(true)
        .interactive(true)
        .build();
    for post in &posts {
        carousel.append(&build_post_widget(post));
    }

    // Tap navigation. Same UX as Stories on every other app: left
    // half goes back, right half advances or closes after the last
    // post. `connect_released` instead of `pressed` so a swipe
    // doesn't accidentally count as a tap.
    {
        let car = carousel.clone();
        let dlg = dialog.clone();
        let click = gtk::GestureClick::new();
        click.set_button(gtk::gdk::BUTTON_PRIMARY);
        click.connect_released(move |gesture, _, x, _| {
            let Some(widget) = gesture.widget() else { return };
            let width = widget.width() as f64;
            let n = car.n_pages();
            if n == 0 {
                return;
            }
            let current = car.position().round() as u32;
            if x < width / 2.0 {
                if current > 0 {
                    let prev = car.nth_page(current - 1);
                    car.scroll_to(&prev, true);
                }
            } else if current + 1 < n {
                let next = car.nth_page(current + 1);
                car.scroll_to(&next, true);
            } else {
                dlg.close();
            }
        });
        carousel.add_controller(click);
    }

    // Body assembly: progress bars above carousel.
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    body.append(&progress_box);
    body.append(&carousel);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&body));
    dialog.set_child(Some(&toolbar));

    install_auto_advance(&carousel, bars, &dialog);

    tracing::info!(
        author = %author_name,
        posts = posts.len(),
        "[stories] presenting dialog",
    );
    dialog.present(Some(parent.upcast_ref::<gtk::Widget>()));
}

fn build_post_widget(post: &MessageRow) -> gtk::Widget {
    let outer = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .vexpand(true)
        .hexpand(true)
        .valign(gtk::Align::Fill)
        .halign(gtk::Align::Fill)
        .build();

    match post.message_type.as_str() {
        "image" | "sticker" => {
            if let Some(path) = post.media_path.clone() {
                // Decode through glycin (sandboxed). Status posts
                // come from arbitrary contacts; we don't want a
                // malformed sticker reaching GdkPixbuf.
                let pic = gtk::Picture::new();
                pic.set_can_shrink(true);
                pic.set_content_fit(gtk::ContentFit::Contain);
                pic.set_hexpand(true);
                pic.set_vexpand(true);
                outer.append(&pic);
                glib::MainContext::default().spawn_local({
                    let pic = pic.clone();
                    async move {
                        let file = gtk::gio::File::for_path(&path);
                        let loader = glycin::Loader::new(file);
                        let result: Option<(glycin::Image, glycin::Frame)> = async {
                            let image = loader.load().await.ok()?;
                            let first = image.next_frame().await.ok()?;
                            Some((image, first))
                        }
                        .await;
                        if let Some((image, first)) = result {
                            let paintable = crate::components::message_media::AnimatedImagePaintable::new(
                                image, first, true,
                            );
                            pic.set_paintable(Some(&paintable));
                        }
                    }
                });
            } else {
                outer.append(&loading_widget(&fl!("stories-photo-downloading")));
            }
        }
        "video" => {
            if let Some(path) = post.media_path.as_deref() {
                let video = gtk::Video::for_filename(Some(std::path::Path::new(path)));
                video.set_autoplay(true);
                video.set_hexpand(true);
                video.set_vexpand(true);
                outer.append(&video);
            } else {
                outer.append(&loading_widget(&fl!("stories-video-downloading")));
            }
        }
        _ => {
            let lbl = gtk::Label::builder()
                .label(post.content.as_deref().unwrap_or(&fl!("stories-status-update")))
                .wrap(true)
                .justify(gtk::Justification::Center)
                .halign(gtk::Align::Center)
                .valign(gtk::Align::Center)
                .build();
            lbl.add_css_class("title-1");
            outer.append(&lbl);
        }
    }
    // Caption: only show real text the user typed alongside their
    // status. The DB stores placeholder strings like "[Image]" /
    // "[Video]" as `content` for media messages with no caption —
    // skip those so the bottom of the carousel doesn't read as a
    // weird Markdown-ish artefact.
    if let Some(caption) = post.content.as_deref()
        && !caption.is_empty()
        && !matches!(post.message_type.as_str(), "text" | "")
        && !is_media_placeholder(caption)
    {
        let cap = gtk::Label::builder()
            .label(caption)
            .wrap(true)
            .justify(gtk::Justification::Center)
            .halign(gtk::Align::Center)
            .margin_top(8)
            .margin_bottom(12)
            .build();
        cap.add_css_class("dim-label");
        outer.append(&cap);
    }
    outer.upcast()
}

/// `[Image]` / `[Video]` / `[Sticker]` / `[Audio]` / `[Document]` —
/// the placeholder content the worker sets on media messages that
/// arrived without a caption. We treat them as "no caption" because
/// they're never meant to be user-facing strings.
fn is_media_placeholder(s: &str) -> bool {
    let t = s.trim();
    matches!(
        t,
        "[Image]" | "[Photo]" | "[Video]" | "[Sticker]" | "[Audio]" | "[Document]" | "[GIF]"
    )
}

/// Centred spinner + text — shown in place of the actual media when
/// the file hasn't been downloaded yet.
fn loading_widget(text: &str) -> gtk::Box {
    let b = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .vexpand(true)
        .hexpand(true)
        .build();
    let spinner = gtk::Spinner::builder()
        .spinning(true)
        .width_request(32)
        .height_request(32)
        .build();
    let label = gtk::Label::new(Some(text));
    label.add_css_class("dim-label");
    b.append(&spinner);
    b.append(&label);
    b
}

/// Auto-advance state machine. A single `Cell<u64>` of elapsed
/// milliseconds drives both the active bar's fraction and the
/// rollover. Reset on user-driven page change (the carousel's
/// `position` notify) so manual swipes restart the dwell.
fn install_auto_advance(
    carousel: &adw::Carousel,
    bars: Vec<gtk::ProgressBar>,
    dialog: &adw::Dialog,
) {
    let elapsed = Rc::new(Cell::new(0u64));
    // Reset when the user manually scrolls (swipe / tap) — the
    // carousel reports a fractional position during the animation,
    // so we settle for whole-page changes by rounding.
    {
        let elapsed = elapsed.clone();
        let last = Rc::new(Cell::new(0u32));
        carousel.connect_position_notify(move |c| {
            let cur = c.position().round() as u32;
            if cur != last.get() {
                last.set(cur);
                elapsed.set(0);
            }
        });
    }

    let bars = Rc::new(bars);
    for bar in bars.iter() {
        bar.set_fraction(0.0);
    }

    let car_weak = carousel.downgrade();
    let dlg_weak = dialog.downgrade();
    glib::timeout_add_local(std::time::Duration::from_millis(TICK_MS), move || {
        // Both upgrades must succeed — if the dialog was dismissed
        // we tear down the timer to stop spinning the CPU.
        let Some(car) = car_weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if dlg_weak.upgrade().is_none() {
            return glib::ControlFlow::Break;
        }
        let n = car.n_pages();
        if n == 0 {
            return glib::ControlFlow::Continue;
        }
        let current = car.position().round() as usize;

        // Past + future bars: snap to 1.0 / 0.0. Cheaper than
        // tracking which bars we last touched, and the visible diff
        // is invisible for a 50 ms tick.
        for (i, bar) in bars.iter().enumerate() {
            if i < current {
                bar.set_fraction(1.0);
            } else if i > current {
                bar.set_fraction(0.0);
            }
        }

        let now = elapsed.get() + TICK_MS;
        let frac = (now as f64 / AUTO_ADVANCE_MS as f64).clamp(0.0, 1.0);
        if let Some(bar) = bars.get(current) {
            bar.set_fraction(frac);
        }

        if now >= AUTO_ADVANCE_MS {
            elapsed.set(0);
            if (current as u32) + 1 < n {
                let next = car.nth_page(current as u32 + 1);
                car.scroll_to(&next, true);
            } else if let Some(dlg) = dlg_weak.upgrade() {
                dlg.close();
                return glib::ControlFlow::Break;
            }
        } else {
            elapsed.set(now);
        }
        glib::ControlFlow::Continue
    });
}
