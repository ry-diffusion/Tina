// Preferences dialog (AdwPreferencesDialog). Three pages:
//   * General — download method + language
//   * Storage — disk usage breakdown + Repair + clear-cache
//   * About   — version + segmented memory bar

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use crate::fl;

use adw::prelude::*;
use relm4::prelude::*;

// RGB colour triples for the two segmented bars.
const COLOR_DB: [f64; 3] = [0.22, 0.52, 0.84];      // blue   — database
const COLOR_MEDIA: [f64; 3] = [0.20, 0.82, 0.48];   // green  — media
const COLOR_AVATARS: [f64; 3] = [1.00, 0.47, 0.00]; // orange — avatars
const COLOR_GTK: [f64; 3] = [0.57, 0.25, 0.67];     // purple — interface
const COLOR_NANACHI: [f64; 3] = [0.13, 0.63, 0.64]; // teal   — service

type BarSegments = Vec<(f64, [f64; 3])>;

/// Persisted under `settings.download_method`. Default is `OnDemand`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DownloadMethod {
    #[default]
    OnDemand,
    Manual,
    Eager,
}

impl DownloadMethod {
    pub const KEY: &'static str = "download_method";

    pub fn as_str(self) -> &'static str {
        match self {
            Self::OnDemand => "on_demand",
            Self::Manual => "manual",
            Self::Eager => "eager",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "manual" => Self::Manual,
            "eager" => Self::Eager,
            _ => Self::OnDemand,
        }
    }

    fn position(self) -> u32 {
        match self {
            Self::OnDemand => 0,
            Self::Manual => 1,
            Self::Eager => 2,
        }
    }

    fn from_position(pos: u32) -> Self {
        match pos {
            1 => Self::Manual,
            2 => Self::Eager,
            _ => Self::OnDemand,
        }
    }
}

/// User's explicit language preference. `System` means "follow the OS locale".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LanguagePref {
    #[default]
    System,
    En,
    PtBr,
}

impl LanguagePref {
    pub fn as_locale(self) -> Option<&'static str> {
        match self {
            Self::System => None,
            Self::En => Some("en-US"),
            Self::PtBr => Some("pt-BR"),
        }
    }

    pub fn to_file_str(self) -> &'static str {
        match self {
            Self::System => "",
            Self::En => "en-US",
            Self::PtBr => "pt-BR",
        }
    }

    pub fn from_file_str(s: &str) -> Self {
        match s.trim() {
            "en-US" => Self::En,
            "pt-BR" => Self::PtBr,
            _ => Self::System,
        }
    }

    fn position(self) -> u32 {
        match self {
            Self::System => 0,
            Self::En => 1,
            Self::PtBr => 2,
        }
    }

    fn from_position(pos: u32) -> Self {
        match pos {
            1 => Self::En,
            2 => Self::PtBr,
            _ => Self::System,
        }
    }
}

#[derive(Debug)]
pub struct SettingsInit {
    pub data_dir: PathBuf,
}

#[derive(Debug)]
pub enum SettingsInput {
    Refresh,
    SetDownloadMethod(DownloadMethod),
    SetNanachiPid(Option<u32>),
    PickDownloadMethod(DownloadMethod),
    PickLanguage(LanguagePref),
    Repair,
    ClearMedia,
    ClearAvatars,
}

#[derive(Debug)]
pub enum SettingsOutput {
    SetDownloadMethod(DownloadMethod),
    Repair,
    ClearMedia,
    ClearAvatars,
    SetLanguage(String),
}

pub struct Settings {
    data_dir: PathBuf,
    download_method: DownloadMethod,
    language_pref: LanguagePref,
    total_size: u64,
    db_size: u64,
    media_size: u64,
    avatars_size: u64,
    self_rss: u64,
    nanachi_rss: Option<u64>,
    nanachi_pid: Option<u32>,
    suppress_combo: std::cell::Cell<bool>,
    suppress_language_combo: std::cell::Cell<bool>,
    // Shared with the Cairo draw closures.
    storage_bar_data: Rc<RefCell<BarSegments>>,
    memory_bar_data: Rc<RefCell<BarSegments>>,
    // Strong refs so we can call queue_draw() from update().
    storage_bar: Option<gtk::DrawingArea>,
    memory_bar: Option<gtk::DrawingArea>,
}

#[relm4::component(pub)]
impl SimpleComponent for Settings {
    type Init = SettingsInit;
    type Input = SettingsInput;
    type Output = SettingsOutput;

    view! {
        #[root]
        adw::PreferencesDialog {
            set_title: &fl!("settings-title"),
            set_search_enabled: false,

            // ── General ──────────────────────────────────────────────
            add = &adw::PreferencesPage {
                set_title: &fl!("settings-general"),
                set_icon_name: Some("preferences-system-symbolic"),

                add = &adw::PreferencesGroup {
                    set_title: &fl!("settings-downloads"),
                    set_description: Some(&fl!("settings-downloads-description")),

                    #[name(method_row)]
                    adw::ComboRow {
                        set_title: &fl!("settings-download-method"),
                        set_subtitle: &fl!("settings-download-method-subtitle"),
                        set_model: Some(&gtk::StringList::new(&[
                            &fl!("settings-download-on-demand"),
                            &fl!("settings-download-manual"),
                            &fl!("settings-download-eager"),
                        ])),
                        #[watch]
                        set_selected: model.download_method.position(),
                        connect_selected_notify[sender, suppress = model.suppress_combo.clone()] => move |row| {
                            if suppress.get() { return; }
                            sender.input(SettingsInput::PickDownloadMethod(
                                DownloadMethod::from_position(row.selected()),
                            ));
                        },
                    },
                },

                add = &adw::PreferencesGroup {
                    set_title: &fl!("settings-language-group"),

                    #[name(language_row)]
                    adw::ComboRow {
                        set_title: &fl!("settings-language"),
                        set_subtitle: &fl!("settings-language-subtitle"),
                        set_model: Some(&gtk::StringList::new(&[
                            &fl!("settings-language-system"),
                            &fl!("settings-language-en"),
                            &fl!("settings-language-pt-br"),
                        ])),
                        #[watch]
                        set_selected: model.language_pref.position(),
                        connect_selected_notify[sender, suppress = model.suppress_language_combo.clone()] => move |row| {
                            if suppress.get() { return; }
                            sender.input(SettingsInput::PickLanguage(
                                LanguagePref::from_position(row.selected()),
                            ));
                        },
                    },
                },
            },

            // ── Storage ──────────────────────────────────────────────
            add = &adw::PreferencesPage {
                set_title: &fl!("settings-storage"),
                set_icon_name: Some("drive-harddisk-symbolic"),

                add = &adw::PreferencesGroup {
                    set_title: &fl!("settings-disk-usage"),
                    #[watch]
                    set_description: Some(&fl!("settings-disk-total",
                        "size" = format_bytes(model.total_size)
                    )),

                    // Segmented storage bar — lives in the group header
                    // area (above the listbox) because it's not a
                    // PreferencesRow. libadwaita inserts it between the
                    // title/description box and the row list.
                    add = &gtk::Box {
                        set_margin_top: 2,
                        set_margin_bottom: 6,

                        #[name(storage_bar_da)]
                        gtk::DrawingArea {
                            set_content_height: 14,
                            set_hexpand: true,
                        },
                    },

                    adw::ActionRow {
                        set_title: &fl!("settings-database"),
                        set_subtitle: &fl!("settings-database-subtitle"),
                        add_suffix = &gtk::Label {
                            #[watch]
                            set_label: &format_bytes(model.db_size),
                            add_css_class: "dim-label",
                            set_valign: gtk::Align::Center,
                        },
                    },

                    adw::ActionRow {
                        set_title: &fl!("settings-media"),
                        set_subtitle: &fl!("settings-media-subtitle"),
                        add_suffix = &gtk::Label {
                            #[watch]
                            set_label: &format_bytes(model.media_size),
                            add_css_class: "dim-label",
                            set_valign: gtk::Align::Center,
                        },
                        add_suffix = &gtk::Button {
                            set_icon_name: "user-trash-symbolic",
                            set_tooltip_text: Some(&fl!("settings-clear-media")),
                            set_valign: gtk::Align::Center,
                            add_css_class: "flat",
                            connect_clicked => SettingsInput::ClearMedia,
                        },
                    },

                    adw::ActionRow {
                        set_title: &fl!("settings-avatars"),
                        set_subtitle: &fl!("settings-avatars-subtitle"),
                        add_suffix = &gtk::Label {
                            #[watch]
                            set_label: &format_bytes(model.avatars_size),
                            add_css_class: "dim-label",
                            set_valign: gtk::Align::Center,
                        },
                        add_suffix = &gtk::Button {
                            set_icon_name: "user-trash-symbolic",
                            set_tooltip_text: Some(&fl!("settings-clear-avatars")),
                            set_valign: gtk::Align::Center,
                            add_css_class: "flat",
                            connect_clicked => SettingsInput::ClearAvatars,
                        },
                    },
                },

                add = &adw::PreferencesGroup {
                    set_title: &fl!("settings-maintenance"),

                    adw::ActionRow {
                        set_title: &fl!("settings-repair"),
                        set_subtitle: &fl!("settings-repair-subtitle"),
                        add_suffix = &gtk::Button {
                            set_label: &fl!("settings-repair-run"),
                            set_valign: gtk::Align::Center,
                            add_css_class: "suggested-action",
                            connect_clicked => SettingsInput::Repair,
                        },
                    },
                },
            },

            // ── About ─────────────────────────────────────────────────
            add = &adw::PreferencesPage {
                set_title: &fl!("settings-about"),
                set_icon_name: Some("help-about-symbolic"),

                add = &adw::PreferencesGroup {
                    set_title: &fl!("app-title"),

                    adw::ActionRow {
                        set_title: &fl!("settings-version"),
                        add_suffix = &gtk::Label {
                            set_label: env!("CARGO_PKG_VERSION"),
                            add_css_class: "dim-label",
                        },
                    },
                },

                add = &adw::PreferencesGroup {
                    set_title: &fl!("settings-memory-group"),
                    #[watch]
                    set_description: Some(&format_bytes(
                        model.self_rss + model.nanachi_rss.unwrap_or(0)
                    )),

                    // Segmented memory bar.
                    add = &gtk::Box {
                        set_margin_top: 2,
                        set_margin_bottom: 6,

                        #[name(memory_bar_da)]
                        gtk::DrawingArea {
                            set_content_height: 14,
                            set_hexpand: true,
                        },
                    },

                    adw::ActionRow {
                        set_title: &fl!("settings-memory-gtk"),
                        add_suffix = &gtk::Label {
                            #[watch]
                            set_label: &format_bytes(model.self_rss),
                            add_css_class: "dim-label",
                            set_valign: gtk::Align::Center,
                        },
                        add_prefix = &gtk::Box {
                            set_valign: gtk::Align::Center,
                            set_size_request: (10, 10),
                            add_css_class: "tina-legend-dot",
                            // purple dot for interface
                        },
                    },

                    adw::ActionRow {
                        set_title: &fl!("settings-memory-nanachi"),
                        #[watch]
                        set_subtitle: &if model.nanachi_pid.is_none() {
                            fl!("settings-memory-nanachi-stopped")
                        } else {
                            String::new()
                        },
                        add_suffix = &gtk::Label {
                            #[watch]
                            set_label: &format_bytes(model.nanachi_rss.unwrap_or(0)),
                            add_css_class: "dim-label",
                            set_valign: gtk::Align::Center,
                        },
                        add_prefix = &gtk::Box {
                            set_valign: gtk::Align::Center,
                            set_size_request: (10, 10),
                            add_css_class: "tina-legend-dot-teal",
                            // teal dot for service
                        },
                    },
                },
            },
        }
    }

    fn init(
        init: Self::Init,
        _root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let saved_lang = std::fs::read_to_string(init.data_dir.join("language"))
            .ok()
            .map(|s| LanguagePref::from_file_str(&s))
            .unwrap_or_default();

        let mut model = Settings {
            data_dir: init.data_dir,
            download_method: DownloadMethod::OnDemand,
            language_pref: saved_lang,
            total_size: 0,
            db_size: 0,
            media_size: 0,
            avatars_size: 0,
            self_rss: 0,
            nanachi_rss: None,
            nanachi_pid: None,
            suppress_combo: std::cell::Cell::new(false),
            suppress_language_combo: std::cell::Cell::new(false),
            storage_bar_data: Rc::new(RefCell::new(vec![])),
            memory_bar_data: Rc::new(RefCell::new(vec![])),
            storage_bar: None,
            memory_bar: None,
        };

        let widgets = view_output!();

        // Wire up Cairo draw functions and keep widget handles for queue_draw.
        {
            let data = model.storage_bar_data.clone();
            widgets.storage_bar_da.set_draw_func(move |_, ctx, w, h| {
                draw_bar(ctx, w as f64, h as f64, &data.borrow());
            });
            model.storage_bar = Some(widgets.storage_bar_da.clone());
        }
        {
            let data = model.memory_bar_data.clone();
            widgets.memory_bar_da.set_draw_func(move |_, ctx, w, h| {
                draw_bar(ctx, w as f64, h as f64, &data.borrow());
            });
            model.memory_bar = Some(widgets.memory_bar_da.clone());
        }

        // Do NOT refresh at init — dir_size() walks the filesystem on the
        // main thread and would freeze the startup spinner. The parent calls
        // SettingsInput::Refresh right before presenting the dialog, which
        // is the right time to pay that cost.
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: SettingsInput, sender: ComponentSender<Self>) {
        match msg {
            SettingsInput::Refresh => {
                self.refresh_storage();
                self.refresh_self_rss();
                self.refresh_nanachi_rss();
            }
            SettingsInput::SetDownloadMethod(m) => {
                self.suppress_combo.set(true);
                self.download_method = m;
                self.suppress_combo.set(false);
            }
            SettingsInput::SetNanachiPid(pid) => {
                self.nanachi_pid = pid;
                self.refresh_nanachi_rss();
            }
            SettingsInput::PickDownloadMethod(m) => {
                if self.download_method != m {
                    self.download_method = m;
                    let _ = sender.output(SettingsOutput::SetDownloadMethod(m));
                }
            }
            SettingsInput::PickLanguage(lang) => {
                if self.language_pref != lang {
                    self.language_pref = lang;
                    let _ = sender.output(SettingsOutput::SetLanguage(
                        lang.to_file_str().to_string(),
                    ));
                }
            }
            SettingsInput::Repair => {
                let _ = sender.output(SettingsOutput::Repair);
            }
            SettingsInput::ClearMedia => {
                let _ = sender.output(SettingsOutput::ClearMedia);
            }
            SettingsInput::ClearAvatars => {
                let _ = sender.output(SettingsOutput::ClearAvatars);
            }
        }
    }
}

impl Settings {
    fn refresh_storage(&mut self) {
        self.db_size = file_size(&self.data_dir.join("tina.db"))
            + file_size(&self.data_dir.join("tina.db-wal"))
            + file_size(&self.data_dir.join("tina.db-shm"))
            + file_size(&self.data_dir.join("whatsmeow.db"))
            + file_size(&self.data_dir.join("whatsmeow.db-wal"))
            + file_size(&self.data_dir.join("whatsmeow.db-shm"));
        self.media_size = dir_size(&self.data_dir.join("media"));
        self.avatars_size = dir_size(&self.data_dir.join("avatars"));
        self.total_size = dir_size(&self.data_dir);

        self.update_storage_bar();
    }

    fn refresh_self_rss(&mut self) {
        self.self_rss = read_rss(std::process::id()).unwrap_or(0);
        self.update_memory_bar();
    }

    fn refresh_nanachi_rss(&mut self) {
        self.nanachi_rss = self.nanachi_pid.and_then(read_rss);
        self.update_memory_bar();
    }

    fn update_storage_bar(&self) {
        let total = self.total_size as f64;
        let mut segs: BarSegments = vec![];
        if total > 0.0 {
            let db_f = self.db_size as f64 / total;
            let media_f = self.media_size as f64 / total;
            let avatars_f = self.avatars_size as f64 / total;
            if db_f > 0.005 { segs.push((db_f, COLOR_DB)); }
            if media_f > 0.005 { segs.push((media_f, COLOR_MEDIA)); }
            if avatars_f > 0.005 { segs.push((avatars_f, COLOR_AVATARS)); }
        }
        *self.storage_bar_data.borrow_mut() = segs;
        if let Some(bar) = &self.storage_bar { bar.queue_draw(); }
    }

    fn update_memory_bar(&self) {
        let gtk_rss = self.self_rss as f64;
        let nan_rss = self.nanachi_rss.unwrap_or(0) as f64;
        let total = gtk_rss + nan_rss;
        let mut segs: BarSegments = vec![];
        if total > 0.0 {
            if gtk_rss > 0.0 { segs.push((gtk_rss / total, COLOR_GTK)); }
            if nan_rss > 0.0 { segs.push((nan_rss / total, COLOR_NANACHI)); }
        }
        *self.memory_bar_data.borrow_mut() = segs;
        if let Some(bar) = &self.memory_bar { bar.queue_draw(); }
    }
}

// ── Cairo drawing ─────────────────────────────────────────────────────────────

fn draw_bar(ctx: &gtk::cairo::Context, w: f64, h: f64, segments: &BarSegments) {
    let r = h / 2.0;

    ctx.save().ok();

    // Clip everything to a fully-rounded pill shape.
    pill(ctx, 0.0, 0.0, w, h, r);
    ctx.clip();

    // Unfilled background.
    ctx.set_source_rgba(0.5, 0.5, 0.5, 0.18);
    ctx.paint().ok();

    // Coloured segments left-to-right.
    let mut x = 0.0_f64;
    for (frac, [r, g, b]) in segments {
        let seg_w = frac * w;
        if seg_w < 0.5 { x += seg_w; continue; }
        ctx.rectangle(x, 0.0, seg_w, h);
        ctx.set_source_rgb(*r, *g, *b);
        ctx.fill().ok();
        x += seg_w;
    }

    ctx.restore().ok();
}

/// Rounded-rectangle path (radius r, assumed ≤ min(w,h)/2).
fn pill(ctx: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    use std::f64::consts::PI;
    ctx.new_sub_path();
    ctx.arc(x + r,     y + r,     r, PI,       3.0 * PI / 2.0);
    ctx.arc(x + w - r, y + r,     r, -PI / 2.0, 0.0);
    ctx.arc(x + w - r, y + h - r, r, 0.0,       PI / 2.0);
    ctx.arc(x + r,     y + h - r, r, PI / 2.0,  PI);
    ctx.close_path();
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else { return 0; };
    let mut total = 0u64;
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue; };
        if meta.is_dir() {
            total += dir_size(&entry.path());
        } else {
            total += meta.len();
        }
    }
    total
}

fn read_rss(pid: u32) -> Option<u64> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kb: u64 = rest.trim().split_whitespace().next()?.parse().ok()?;
            return Some(kb * 1024);
        }
    }
    None
}

fn format_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{n} B") } else { format!("{:.1} {}", v, UNITS[i]) }
}
