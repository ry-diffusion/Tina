// Fluent-based localisation. Call `init()` once from `main()` before
// building the UI. Afterwards use the `fl!` macro anywhere in the crate.
//
// Supported locales: en-US (fallback), pt-BR.
// The system locale is detected via `sys-locale`; any `pt*` locale maps
// to pt-BR, everything else falls back to en-US.
//
// FTL files are embedded at compile time via `rust-embed`.

use std::cell::OnceCell;

use fluent_bundle::{FluentArgs, FluentBundle, FluentResource};
use rust_embed::RustEmbed;
use unic_langid::LanguageIdentifier;

#[derive(RustEmbed)]
#[folder = "i18n/"]
struct I18nAssets;

thread_local! {
    static BUNDLE: OnceCell<FluentBundle<FluentResource>> = const { OnceCell::new() };
}

fn load_ftl(filename: &str) -> String {
    I18nAssets::get(filename)
        .and_then(|f| std::str::from_utf8(f.data.as_ref()).ok().map(str::to_string))
        .unwrap_or_default()
}

fn build_bundle(locale: &str) -> FluentBundle<FluentResource> {
    let (ftl_file, langid_str) = if locale.starts_with("pt") {
        ("pt-BR.ftl", "pt-BR")
    } else {
        ("en-US.ftl", "en-US")
    };

    let langid: LanguageIdentifier = langid_str.parse().expect("valid langid");
    let mut bundle = FluentBundle::new(vec![langid]);

    let ftl = load_ftl(ftl_file);
    let res = FluentResource::try_new(ftl).expect("valid FTL");
    bundle.add_resource(res).expect("no duplicate messages");

    // For missing keys fall back to en-US without overriding existing ones.
    // `add_resource` (not `add_resource_overriding`) skips keys already
    // present, so pt-BR messages are never replaced by their English counterparts.
    if locale.starts_with("pt") {
        let en_langid: LanguageIdentifier = "en-US".parse().expect("valid langid");
        bundle.locales.push(en_langid);
        let en_res = FluentResource::try_new(load_ftl("en-US.ftl")).expect("valid FTL");
        // Intentionally ignore errors — they fire for every key that already
        // exists in pt-BR, which is the expected case for a fully translated file.
        let _ = bundle.add_resource(en_res);
    }

    bundle
}

/// Must be called once from `main()` before any UI is built.
/// Pass `Some(locale)` to override the system locale (e.g. from a saved preference).
/// `None` detects the system locale via `sys-locale`.
pub fn init(override_locale: Option<String>) {
    let locale = override_locale.unwrap_or_else(|| {
        sys_locale::get_locale().unwrap_or_else(|| "en-US".to_string())
    });
    tracing::debug!("i18n: using locale {locale:?}");
    BUNDLE.with(|cell| {
        cell.set(build_bundle(&locale)).ok();
    });
}

#[inline]
fn with_bundle<R>(f: impl FnOnce(&FluentBundle<FluentResource>) -> R) -> R {
    BUNDLE.with(|cell| {
        let bundle = cell.get_or_init(|| build_bundle("en-US"));
        f(bundle)
    })
}

pub fn get(key: &str) -> String {
    with_bundle(|bundle| {
        let Some(msg) = bundle.get_message(key) else {
            return key.to_string();
        };
        let Some(pattern) = msg.value() else {
            return key.to_string();
        };
        let mut errors = vec![];
        bundle.format_pattern(pattern, None, &mut errors).to_string()
    })
}

pub fn get_args(key: &str, build: impl FnOnce(&mut FluentArgs<'static>)) -> String {
    with_bundle(|bundle| {
        let Some(msg) = bundle.get_message(key) else {
            return key.to_string();
        };
        let Some(pattern) = msg.value() else {
            return key.to_string();
        };
        let mut args: FluentArgs<'static> = FluentArgs::new();
        build(&mut args);
        let mut errors = vec![];
        bundle
            .format_pattern(pattern, Some(&args), &mut errors)
            .to_string()
    })
}

/// Translate a Fluent message key.
///
/// ```
/// fl!("app-title")
/// fl!("toast-disconnected", "reason" = reason_str)
/// fl!("preview-voice-duration", "min" = mins_str, "sec" = secs_str)
/// ```
#[macro_export]
macro_rules! fl {
    ($key:expr) => {
        $crate::i18n::get($key)
    };
    ($key:expr, $($name:literal = $val:expr),+ $(,)?) => {
        $crate::i18n::get_args($key, |_args| {
            $( _args.set($name, fluent_bundle::FluentValue::from(
                $val.to_string()
            )); )+
        })
    };
}
