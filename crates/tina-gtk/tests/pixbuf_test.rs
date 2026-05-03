#[test]
fn test_webp_support() {
    let formats = gtk::gdk_pixbuf::Pixbuf::formats();
    let has_webp = formats.iter().any(|f| f.name().as_deref() == Some("webp"));
    println!("Has WebP: {}", has_webp);
}
