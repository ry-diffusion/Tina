use crate::fl;
// Static placeholder pages for the Init / Error scenes. The
// Syncing and Repairing scenes are now defined inline in the
// view! macro so they can bind reactively to AppModel state.

pub fn init_page() -> adw::StatusPage {
    let page = adw::StatusPage::builder()
        .icon_name("chat-bubble-dots-symbolic")
        .title(&fl!("app-title"))
        .description(&fl!("init-page-description"))
        .build();
    let spinner = gtk::Spinner::builder().spinning(true).build();
    page.set_child(Some(&spinner));
    page
}

pub fn error_page(msg: String) -> adw::StatusPage {
    adw::StatusPage::builder()
        .icon_name("dialog-error-symbolic")
        .title(&fl!("error-page-title"))
        .description(&msg)
        .build()
}
