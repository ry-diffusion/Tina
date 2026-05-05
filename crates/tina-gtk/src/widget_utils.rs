// Helpers for working with GTK widgets, in the spirit of Fractal's
// `utils::ChildExt` (`/home/zesmoi/src/Reference/fractal/src/utils/mod.rs:605`).
//
// `child_or_else` / `child_or_default` are the recycling primitive used
// when a parent widget hosts a polymorphic child: instead of removing
// the existing child and constructing a new one on every rebind, we
// downcast-check first and reuse the existing widget when it's the right
// type. Saves widget churn when the same row is recycled across items
// of the same kind (e.g. a `ListView` factory reusing a row for two
// different messages of the same content type).

use adw::prelude::*;

/// Trait giving any GTK widget a `child_or_else` / `child_or_default`
/// method. The host widget must own a single child (e.g. a `gtk::Bin`-
/// like role: `gtk::Frame`, `gtk::Button`, `gtk::ListItem`,
/// `gtk::Box` with one child, etc.).
pub trait ChildContainer {
    /// Read the current child (if any) without taking ownership.
    fn current_child(&self) -> Option<gtk::Widget>;
    /// Replace the current child.
    fn replace_child(&self, child: &gtk::Widget);
}

impl ChildContainer for gtk::ListItem {
    fn current_child(&self) -> Option<gtk::Widget> {
        gtk::prelude::ListItemExt::child(self)
    }
    fn replace_child(&self, child: &gtk::Widget) {
        gtk::prelude::ListItemExt::set_child(self, Some(child));
    }
}

impl ChildContainer for gtk::Frame {
    fn current_child(&self) -> Option<gtk::Widget> {
        gtk::prelude::FrameExt::child(self)
    }
    fn replace_child(&self, child: &gtk::Widget) {
        gtk::prelude::FrameExt::set_child(self, Some(child));
    }
}

/// Returns the current child if it's already of type `W`; otherwise
/// constructs one with `f`, installs it, and returns the new child.
/// Mirrors Fractal's `child_or_else`.
pub fn child_or_else<C, W>(host: &C, f: impl FnOnce() -> W) -> W
where
    C: ChildContainer,
    W: glib::object::IsA<gtk::Widget> + Clone,
{
    if let Some(existing) = host.current_child()
        && let Ok(typed) = existing.downcast::<W>()
    {
        return typed;
    }
    let fresh = f();
    host.replace_child(fresh.upcast_ref::<gtk::Widget>());
    fresh
}

/// Default-constructed variant — use when `W: Default`.
#[allow(dead_code)]
pub fn child_or_default<C, W>(host: &C) -> W
where
    C: ChildContainer,
    W: glib::object::IsA<gtk::Widget> + Clone + Default,
{
    child_or_else(host, W::default)
}
