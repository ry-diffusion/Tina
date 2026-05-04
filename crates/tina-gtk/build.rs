fn main() {
    relm4_icons_build::bundle_icons(
        "icon_names.rs",
        Some("dev.tina.Tina"),
        None::<&str>,
        None::<&str>,
        [
            "curved-arrow-left",
            // Init page brand mark + status pages.
            "chat-bubble-dots",
            "loop",
            "wrench",
            // Compose-bar icons. The host icon theme can't be relied
            // on (the user's screenshot showed an empty rectangle
            // where `smile-add-symbolic` should have been), so we
            // bundle our own.
            "attachment",
            "sticker-regular",
            "emoji-regular",
            "mic-3",
            "paper-plane",
            // Delivery-status icons next to outgoing bubbles.
            "clock-loader-40",
            "check",
            "done-all",
            "warning",
        ],
    );
}
