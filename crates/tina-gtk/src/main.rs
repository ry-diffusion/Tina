// Tina GTK4/libadwaita frontend, built on top of tina-worker (whatsmeow bridge).
//
// Architecture:
//   * `relm4` drives all UI; `AdwApplication` owns the lifecycle.
//   * The whatsmeow bridge / DB live behind `service::ServiceHandle`, which
//     runs a tokio runtime on a dedicated thread and exposes a sync-friendly
//     command channel + a relm4 Sender to push `AppMsg` back to the UI.
//   * `app::AppModel` is the root component; it owns the navigation between
//     the Init/QRLogin/Syncing/InApp/Error pages and forwards user commands
//     to the service worker.

mod app;
mod banner;
mod components;
mod format;
mod inventory;
mod qr;
mod service;
mod time;
mod wa_id;
#[allow(dead_code)]
mod widget_utils;

/// Constants for the icons we bundle via relm4-icons-build (see `build.rs`).
/// Both shipped (icon-development-kit feature) and custom icons are loaded
/// into the GResource embedded in the binary.
pub mod icon_names {
    include!(concat!(env!("OUT_DIR"), "/icon_names.rs"));
}

use std::path::PathBuf;

use color_eyre::eyre::Context;
use relm4::RelmApp;
use tracing_subscriber::EnvFilter;

use crate::banner::print_banner;

const APP_ID: &str = "br.com.zesmoi.Tina";

fn find_nanachi_dir() -> color_eyre::Result<PathBuf> {
    let exe_path = std::env::current_exe()?;
    let mut current = exe_path.parent();
    while let Some(dir) = current {
        let nanachi = dir.join("nanachi");
        if nanachi.join("go.mod").exists() {
            return Ok(nanachi);
        }
        current = dir.parent();
    }
    let cwd = std::env::current_dir()?;
    let nanachi = cwd.join("nanachi");
    if nanachi.join("go.mod").exists() {
        return Ok(nanachi);
    }
    Err(color_eyre::eyre::eyre!(
        "Could not find nanachi directory. Run from the project root."
    ))
}

fn main() -> color_eyre::Result<()> {
    print_banner();

    tracing_subscriber::fmt()
        .with_thread_names(true)
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new("tina_gtk=info,tina_core=info,tina_worker=info,tina_db=info")
        }))
        .pretty()
        .init();

    tracing::info!("Tina (GTK) start");

    let nanachi_dir = find_nanachi_dir().wrap_err("locating nanachi dir")?;

    let app = RelmApp::new(APP_ID);
    relm4_icons::initialize_icons(icon_names::GRESOURCE_BYTES, icon_names::RESOURCE_PREFIX);
    relm4::set_global_css(&format!(
        "{}\n{}",
        components::message_bubble::MESSAGE_ROW_CSS,
        components::chat_row::CHAT_ROW_CSS,
    ));
    app.run::<app::AppModel>(app::AppInit { nanachi_dir });
    Ok(())
}
