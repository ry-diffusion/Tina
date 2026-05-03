fn main() {
    relm4_icons_build::bundle_icons(
        "icon_names.rs",
        Some("dev.tina.Tina"),
        None::<&str>,
        None::<&str>,
        ["curved-arrow-left"],
    );
}
