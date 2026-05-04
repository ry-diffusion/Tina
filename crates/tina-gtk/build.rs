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
        ],
    );
}
