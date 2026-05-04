// Static placeholder pages for the Init / Syncing / Error scenes. The
// real chat UI lives in `MainPage`; these are just simple AdwStatusPages.

pub fn init_page() -> adw::StatusPage {
    let page = adw::StatusPage::builder()
        .icon_name("chat-bubble-text-symbolic")
        .title("Tina")
        .description("Initialising…")
        .build();
    let spinner = gtk::Spinner::builder().spinning(true).build();
    page.set_child(Some(&spinner));
    page
}

pub fn syncing_page() -> adw::StatusPage {
    let page = adw::StatusPage::builder()
        .icon_name("emblem-synchronizing-symbolic")
        .title("Syncing messages")
        .description("Hang on while we pull your history.")
        .build();
    let spinner = gtk::Spinner::builder().spinning(true).build();
    page.set_child(Some(&spinner));
    page
}

pub fn error_page(msg: String) -> adw::StatusPage {
    adw::StatusPage::builder()
        .icon_name("dialog-error-symbolic")
        .title("Something went wrong")
        .description(&msg)
        .build()
}
