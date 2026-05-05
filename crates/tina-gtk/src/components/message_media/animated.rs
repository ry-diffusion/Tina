// `AnimatedImagePaintable` — a `gdk::Paintable` subclass that wraps a
// glycin `Image` decoder + the current `Frame`, ticking through the
// animation on a glib timeout. Direct port of Fractal's
// `components/media/animated_image_paintable.rs`, simplified:
//
//   • No `CountedRef` shared state. The owning widget calls
//     `set_playing(true/false)` on map/unmap so the animation only
//     ticks while the row is on-screen. With virtualisation that's
//     ~10–20 simultaneous timers max — a non-issue.
//   • Frame loading uses glib's tokio integration since we already
//     run a tokio runtime via the worker.
//
// The paintable starts with `Some(first_frame)` after init; before
// the first frame loads we don't make the paintable available to
// the widget at all.

use std::cell::{Cell, OnceCell, RefCell};

use glib::clone;
use glib::subclass::prelude::*;
use gtk::gdk;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct AnimatedImagePaintable {
        /// The glycin decoder. `OnceCell` because we set it once at
        /// `init` and never rotate it.
        pub(super) decoder: OnceCell<glycin::Image>,
        /// Frame currently being painted.
        pub(super) current_frame: RefCell<Option<glycin::Frame>>,
        /// Pre-loaded next frame, swapped in by the timer.
        pub(super) next_frame: RefCell<Option<glycin::Frame>>,
        /// Active glib timeout source for the frame swap.
        pub(super) timeout: RefCell<Option<glib::SourceId>>,
        /// `true` when the parent widget is on-screen. Toggled via
        /// `set_playing`. Fractal uses a CountedRef; we keep it
        /// simple because Tina's parent widget tracks
        /// map/unmap directly.
        pub(super) playing: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AnimatedImagePaintable {
        const NAME: &'static str = "TinaAnimatedImagePaintable";
        type Type = super::AnimatedImagePaintable;
        type Interfaces = (gdk::Paintable,);
    }

    impl ObjectImpl for AnimatedImagePaintable {}

    impl PaintableImpl for AnimatedImagePaintable {
        fn intrinsic_height(&self) -> i32 {
            self.current_frame
                .borrow()
                .as_ref()
                .map(|f| f.height())
                .or_else(|| self.decoder.get().map(|d| d.details().height()))
                .and_then(|h| i32::try_from(h).ok())
                .unwrap_or(0)
        }

        fn intrinsic_width(&self) -> i32 {
            self.current_frame
                .borrow()
                .as_ref()
                .map(|f| f.width())
                .or_else(|| self.decoder.get().map(|d| d.details().width()))
                .and_then(|w| i32::try_from(w).ok())
                .unwrap_or(0)
        }

        fn snapshot(&self, snapshot: &gdk::Snapshot, width: f64, height: f64) {
            if let Some(frame) = self.current_frame.borrow().as_ref() {
                frame.texture().snapshot(snapshot, width, height);
            }
        }

        fn flags(&self) -> gdk::PaintableFlags {
            gdk::PaintableFlags::empty()
        }
    }
}

glib::wrapper! {
    pub struct AnimatedImagePaintable(ObjectSubclass<imp::AnimatedImagePaintable>)
        @implements gdk::Paintable;
}

impl AnimatedImagePaintable {
    /// Construct from a fully-initialised decoder + the first frame.
    /// `start_playing` controls whether animation begins immediately
    /// (typically `true` if the parent widget is already mapped, else
    /// `false`).
    pub fn new(
        decoder: glycin::Image,
        first_frame: glycin::Frame,
        start_playing: bool,
    ) -> Self {
        let obj = glib::Object::new::<Self>();
        let imp = obj.imp();
        imp.decoder
            .set(decoder)
            .map_err(|_| ())
            .expect("decoder set once");
        *imp.current_frame.borrow_mut() = Some(first_frame);
        if start_playing {
            obj.set_playing(true);
        }
        obj
    }

    /// Toggle animation. `false` parks the timer; `true` re-arms it.
    /// Cheap to call, so wire to map/unmap.
    pub fn set_playing(&self, playing: bool) {
        let imp = self.imp();
        let was = imp.playing.replace(playing);
        if was == playing {
            return;
        }
        if !playing {
            if let Some(source) = imp.timeout.take() {
                source.remove();
            }
            return;
        }
        // Resuming — schedule the next swap if we have a delay.
        self.schedule_next_frame();
    }

    fn schedule_next_frame(&self) {
        let imp = self.imp();
        if !imp.playing.get() {
            return;
        }
        if imp.timeout.borrow().is_some() {
            return; // already scheduled
        }
        // Pull the delay from the current frame; if absent, no
        // animation (single-frame image).
        let Some(delay) = imp
            .current_frame
            .borrow()
            .as_ref()
            .and_then(|f| f.delay())
        else {
            return;
        };
        let source = glib::timeout_add_local_once(
            delay,
            clone!(
                #[weak(rename_to = obj)]
                self,
                move || {
                    obj.advance_frame();
                }
            ),
        );
        *imp.timeout.borrow_mut() = Some(source);

        // Kick off the next-frame load in the background. On
        // success we stash it into `next_frame`, ready for the
        // timer to swap.
        glib::MainContext::default().spawn_local(clone!(
            #[weak(rename_to = obj)]
            self,
            async move {
                obj.load_next_frame().await;
            }
        ));
    }

    fn advance_frame(&self) {
        let imp = self.imp();
        // Drop the timeout source; we're firing now.
        let _ = imp.timeout.take();

        let Some(next) = imp.next_frame.take() else {
            // Loader hasn't caught up — wait for `load_next_frame`
            // to land and re-schedule from there.
            return;
        };
        *imp.current_frame.borrow_mut() = Some(next);
        self.invalidate_contents();
        self.schedule_next_frame();
    }

    async fn load_next_frame(&self) {
        let imp = self.imp();
        let Some(decoder) = imp.decoder.get() else {
            return;
        };
        match decoder.next_frame().await {
            Ok(frame) => {
                *imp.next_frame.borrow_mut() = Some(frame);
                // If the timer already fired and was waiting for us,
                // run the swap now.
                if imp.timeout.borrow().is_none() {
                    self.advance_frame();
                }
            }
            Err(e) => {
                tracing::debug!(error = %e, "AnimatedImagePaintable: next_frame failed");
            }
        }
    }
}
