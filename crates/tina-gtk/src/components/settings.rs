// Preferences dialog (AdwPreferencesDialog). Three pages:
//   * General — download method (on-demand / manual / eager)
//   * Storage — disk usage breakdown + Repair button + clear-cache
//   * About   — app version + RSS for tina-gtk and the nanachi child
//
// This component owns the dialog widget; the parent presents it via
// `dialog.present(parent_window)` whenever the user opens it from the
// profile menu. Storage usage and RSS are recomputed on each open
// (signalled by `SettingsInput::Refresh`) so the numbers stay fresh
// without a recurring timer eating CPU while the dialog is hidden.

use std::path::{Path, PathBuf};

use adw::prelude::*;
use relm4::prelude::*;

/// Persisted under `settings.download_method`. Default is `OnDemand`,
/// matching the current behaviour: media is downloaded only when the
/// user clicks the placeholder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadMethod {
    /// Click-to-download (current default).
    OnDemand,
    /// No automatic downloads at all — user must trigger every fetch
    /// from the message context menu. Useful on metered networks.
    Manual,
    /// Auto-download every incoming media payload as it arrives. Costs
    /// bandwidth but means images render immediately.
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

#[derive(Debug)]
pub struct SettingsInit {
    /// Root of the per-user data dir (`~/.local/share/tina/`). Used to
    /// compute disk usage rows. Pulled from `directories::ProjectDirs`
    /// at init time so the dialog stays self-contained.
    pub data_dir: PathBuf,
}

#[derive(Debug)]
pub enum SettingsInput {
    /// Recompute storage + RSS rows. Sent by the parent right before
    /// presenting the dialog.
    Refresh,
    /// Worker reported the persisted download method.
    SetDownloadMethod(DownloadMethod),
    /// Worker reported the nanachi PID (or `None` if not running yet).
    SetNanachiPid(Option<u32>),
    /// Internal: ComboRow selection changed.
    PickDownloadMethod(DownloadMethod),
    /// User clicked the "Repair (reconcile)" row.
    Repair,
    /// User clicked "Clear media cache" / "Clear avatar cache".
    ClearMedia,
    ClearAvatars,
}

#[derive(Debug)]
pub enum SettingsOutput {
    /// Persist via the worker.
    SetDownloadMethod(DownloadMethod),
    /// Bubble up to AppMsg::RequestRepair.
    Repair,
    ClearMedia,
    ClearAvatars,
}

pub struct Settings {
    data_dir: PathBuf,
    download_method: DownloadMethod,
    /// Total bytes for `data_dir`, summed recursively.
    total_size: u64,
    db_size: u64,
    media_size: u64,
    avatars_size: u64,
    self_rss: u64,
    nanachi_rss: Option<u64>,
    nanachi_pid: Option<u32>,
    /// Latch for the ComboRow `selected` setter — without it, the
    /// `notify::selected` handler we install fires recursively on
    /// programmatic updates and pushes the same message to the
    /// worker every time we Refresh.
    suppress_combo: std::cell::Cell<bool>,
}

#[relm4::component(pub)]
impl SimpleComponent for Settings {
    type Init = SettingsInit;
    type Input = SettingsInput;
    type Output = SettingsOutput;

    view! {
        #[root]
        adw::PreferencesDialog {
            set_title: "Preferences",
            set_search_enabled: false,

            add = &adw::PreferencesPage {
                set_title: "General",
                set_icon_name: Some("preferences-system-symbolic"),

                add = &adw::PreferencesGroup {
                    set_title: "Downloads",
                    set_description: Some(
                        "When to fetch image, video and audio attachments.",
                    ),

                    #[name(method_row)]
                    adw::ComboRow {
                        set_title: "Download method",
                        set_subtitle: "On-demand: fetched when you tap the placeholder.",

                        set_model: Some(&gtk::StringList::new(&[
                            "On-demand",
                            "Manual",
                            "Eager",
                        ])),
                        #[watch]
                        set_selected: model.download_method.position(),

                        connect_selected_notify[sender, suppress = model.suppress_combo.clone()] => move |row| {
                            if suppress.get() {
                                return;
                            }
                            let pick = DownloadMethod::from_position(row.selected());
                            sender.input(SettingsInput::PickDownloadMethod(pick));
                        },
                    },
                },
            },

            add = &adw::PreferencesPage {
                set_title: "Storage",
                set_icon_name: Some("drive-harddisk-symbolic"),

                add = &adw::PreferencesGroup {
                    set_title: "Disk usage",
                    #[watch]
                    set_description: Some(&format!(
                        "Total: {}",
                        format_bytes(model.total_size),
                    )),

                    // The suffix labels are declared inline (not via a
                    // helper) so the relm4 macro builds the widget once
                    // at init and only updates `set_label` reactively.
                    // Calling `add_suffix` with `#[watch]` and a freshly-
                    // built helper kept appending a new row of labels
                    // on every refresh.
                    adw::ActionRow {
                        set_title: "Database",
                        set_subtitle: "Messages, chats, contacts (tina.db).",
                        add_suffix = &gtk::Label {
                            #[watch]
                            set_label: &format_bytes(model.db_size),
                            add_css_class: "dim-label",
                            set_valign: gtk::Align::Center,
                        },
                    },

                    adw::ActionRow {
                        set_title: "Media",
                        set_subtitle: "Images, videos, audio, documents.",
                        add_suffix = &gtk::Label {
                            #[watch]
                            set_label: &format_bytes(model.media_size),
                            add_css_class: "dim-label",
                            set_valign: gtk::Align::Center,
                        },
                        add_suffix = &gtk::Button {
                            set_icon_name: "user-trash-symbolic",
                            set_tooltip_text: Some("Clear media cache"),
                            set_valign: gtk::Align::Center,
                            add_css_class: "flat",
                            connect_clicked => SettingsInput::ClearMedia,
                        },
                    },

                    adw::ActionRow {
                        set_title: "Avatars",
                        set_subtitle: "Profile pictures cached on disk.",
                        add_suffix = &gtk::Label {
                            #[watch]
                            set_label: &format_bytes(model.avatars_size),
                            add_css_class: "dim-label",
                            set_valign: gtk::Align::Center,
                        },
                        add_suffix = &gtk::Button {
                            set_icon_name: "user-trash-symbolic",
                            set_tooltip_text: Some("Clear avatar cache"),
                            set_valign: gtk::Align::Center,
                            add_css_class: "flat",
                            connect_clicked => SettingsInput::ClearAvatars,
                        },
                    },
                },

                add = &adw::PreferencesGroup {
                    set_title: "Maintenance",

                    adw::ActionRow {
                        set_title: "Repair",
                        set_subtitle:
                            "Re-pull contacts, groups and newsletter metadata \
                             from WhatsApp without re-pairing.",
                        add_suffix = &gtk::Button {
                            set_label: "Run",
                            set_valign: gtk::Align::Center,
                            add_css_class: "suggested-action",
                            connect_clicked => SettingsInput::Repair,
                        },
                    },
                },
            },

            add = &adw::PreferencesPage {
                set_title: "About",
                set_icon_name: Some("help-about-symbolic"),

                add = &adw::PreferencesGroup {
                    set_title: "Tina",

                    adw::ActionRow {
                        set_title: "Version",
                        add_suffix = &gtk::Label {
                            set_label: env!("CARGO_PKG_VERSION"),
                            add_css_class: "dim-label",
                        },
                    },

                    adw::ActionRow {
                        set_title: "Memory (this process)",
                        set_subtitle: "Resident set size of tina-gtk.",
                        add_suffix = &gtk::Label {
                            #[watch]
                            set_label: &format_bytes(model.self_rss),
                            add_css_class: "dim-label",
                            set_valign: gtk::Align::Center,
                        },
                    },

                    adw::ActionRow {
                        set_title: "Memory (nanachi)",
                        #[watch]
                        set_subtitle: &match model.nanachi_pid {
                            Some(pid) => format!("Go subprocess (pid {pid})."),
                            None => "Go subprocess (not running).".to_string(),
                        },
                        add_suffix = &gtk::Label {
                            #[watch]
                            set_label: &format_bytes(model.nanachi_rss.unwrap_or(0)),
                            add_css_class: "dim-label",
                            set_valign: gtk::Align::Center,
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
        let model = Settings {
            data_dir: init.data_dir,
            download_method: DownloadMethod::OnDemand,
            total_size: 0,
            db_size: 0,
            media_size: 0,
            avatars_size: 0,
            self_rss: 0,
            nanachi_rss: None,
            nanachi_pid: None,
            suppress_combo: std::cell::Cell::new(false),
        };
        let widgets = view_output!();
        sender.input(SettingsInput::Refresh);
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
                // Programmatic update — block our own combo handler so
                // the worker doesn't see this echoed back as a write.
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
        // Total walks the whole data dir (catches the .bak files +
        // any future subdirs) — separate from the labelled rows so
        // the breakdown still adds up to something close.
        self.total_size = dir_size(&self.data_dir);
    }

    fn refresh_self_rss(&mut self) {
        self.self_rss = read_rss(std::process::id()).unwrap_or(0);
    }

    fn refresh_nanachi_rss(&mut self) {
        self.nanachi_rss = self.nanachi_pid.and_then(read_rss);
    }
}

/// Total bytes for a regular file (returns 0 if missing/unreadable —
/// the storage panel is informational, not load-bearing).
fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// Recursively-summed size of a directory tree. Skips on the first I/O
/// error rather than aborting; an unreadable subdirectory just gets a
/// short-count, which is fine for the disk-usage row.
fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    let mut total = 0u64;
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            total += dir_size(&entry.path());
        } else {
            total += meta.len();
        }
    }
    total
}

/// Read VmRSS (resident set size) for `pid` from `/proc/<pid>/status`.
/// Returns bytes. None if the file is missing or unparseable.
fn read_rss(pid: u32) -> Option<u64> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            // "VmRSS:    12345 kB"
            let kb: u64 = rest
                .trim()
                .split_whitespace()
                .next()?
                .parse()
                .ok()?;
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
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{:.1} {}", v, UNITS[i])
    }
}

