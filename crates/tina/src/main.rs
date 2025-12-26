use color_eyre::eyre::Context;

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
                .add_directive("tina_core=info".parse().unwrap())
                .add_directive("tina_db=info".parse().unwrap())
                .add_directive("tina_ipc=info".parse().unwrap()),
        )
        .init();

    tracing::info!("TINA START!");

    iced::application(Tina::default, Tina::update, Tina::view)
        .theme(Tina::theme)
        .run()
        .wrap_err("Iced initialization failed")?;
    Ok(())
}
