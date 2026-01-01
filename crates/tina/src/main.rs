use std::path::PathBuf;
mod scenes;
mod state;
use color_eyre::eyre::Context;
use state::{TinaUIServiceWorker, UIMessage};
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::banner::print_banner;

slint::include_modules!();

mod banner;

fn find_nanachi_dir() -> color_eyre::Result<PathBuf> {
    let exe_path = std::env::current_exe()?;

    let mut current = exe_path.parent();
    while let Some(dir) = current {
        let nanachi = dir.join("nanachi");
        if nanachi.join("package.json").exists() {
            return Ok(nanachi);
        }
        current = dir.parent();
    }

    let cwd = std::env::current_dir()?;
    let nanachi = cwd.join("nanachi");
    if nanachi.join("package.json").exists() {
        return Ok(nanachi);
    }

    Err(color_eyre::eyre::eyre!(
        "Could not find nanachi directory. Make sure you're running from the project root."
    ))
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    print_banner();

    tracing_subscriber::fmt()
        .with_thread_names(true)
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("tina=info".parse()?)
                .add_directive("tina_core=info".parse()?)
                .add_directive("tina_worker=info".parse()?)
                .add_directive("tina_db=debug".parse()?),
        )
        .pretty()
        .init();

    info!("Tina start");
    let nanachi_dir = find_nanachi_dir()?;

    let ui = Tina::new().wrap_err("Failed to build Tina UI")?;

    let ui_worker = TinaUIServiceWorker::new(&ui, nanachi_dir);

    // Trigger initialization
    ui_worker
        .send(UIMessage::Initialize)
        .wrap_err("Failed to send initialization message")?;

    ui.run()?;

    ui_worker
        .join()
        .expect("UI Thread Panicked. this shouldn't happen. This is a bug.");
    Ok(())
}
