use color_eyre::eyre::{Context, ContextCompat};
use directories::ProjectDirs;

use crate::app::Tina;

mod app;
mod banner;

fn main() -> color_eyre::Result<()> {
    banner::print_banner();

    tracing_subscriber::fmt()
        .pretty()
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .from_env_lossy()
                .add_directive("tina=info".parse().unwrap())
                .add_directive("tina_worker=info".parse().unwrap())
                .add_directive("tina_ipc=info".parse().unwrap())
                .add_directive("tina_db=info".parse().unwrap()),
        )
        .init();

    tracing::info!("TINA START!");

    let state_dir =
        ProjectDirs::from("com.br", "zesmoi", "tina").wrap_err("Failed to get state directory")?;

    tracing::info!("App folders: {state_dir:?}");

    iced::application(Tina::default, Tina::update, Tina::view)
        .theme(Tina::theme)
        .run()
        .wrap_err("Iced initialization failed")?;
    Ok(())
}
