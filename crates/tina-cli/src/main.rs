// Tina CLI: bare-bones text menu for inspecting accounts, chats and
// messages without the GTK UI. Useful for debugging the worker / DB
// layers in isolation.

mod commands;
mod events;
mod io;

use color_eyre::eyre::{Context, Result};
use tina_worker::TinaWorker;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .from_env_lossy()
                .add_directive("tina_cli=info".parse().unwrap())
                .add_directive("tina_worker=info".parse().unwrap())
                .add_directive("tina_ipc=info".parse().unwrap())
                .add_directive("tina_db=info".parse().unwrap()),
        )
        .init();

    let nanachi_dir = io::find_nanachi_dir()?;

    println!("Nanachi directory: {}", nanachi_dir.display());

    let mut worker = TinaWorker::new(nanachi_dir)
        .await
        .wrap_err("Failed to create worker")?;

    let mut event_rx = worker
        .take_event_receiver()
        .ok_or_else(|| color_eyre::eyre::eyre!("Failed to get event receiver"))?;

    worker.start().await.wrap_err("Failed to start worker")?;

    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            events::handle_event(event);
        }
    });

    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    loop {
        io::print_menu();
        let choice = io::read_line("Choice: ")?;

        match choice.trim() {
            "1" => commands::create_account(&worker).await?,
            "2" => commands::list_accounts(&worker).await?,
            "3" => commands::login_account(&worker).await?,
            "4" => commands::list_contacts(&worker).await?,
            "5" => commands::list_messages(&worker).await?,
            "6" => commands::list_chats(&worker).await?,
            "7" => commands::send_message(&worker).await?,
            "8" => commands::reconcile_account(&worker).await?,
            "0" => {
                println!("Shutting down...");
                worker.stop().await?;
                break;
            }
            _ => println!("Invalid choice"),
        }
    }

    Ok(())
}
