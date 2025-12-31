use std::path::PathBuf;

use color_eyre::eyre::Context;
use owo_colors::OwoColorize;
use slint::ComponentHandle;
use tracing::info;

mod commands;
mod event_loop;
mod state;
mod ui_bridge;

use commands::{Command, create_command_channel, CommandSender};
use event_loop::EventLoop;
use state::create_app_state;
use ui_bridge::{TinaApp, AppState as SlintAppState, UiBridge};

fn print_banner() {
    let banner = r#"
  _____ _             
 |_   _(_)_ __   __ _ 
   | | | | '_ \ / _` |
   | | | | | | | (_| |
   |_| |_|_| |_|\__,_|
                      
    WhatsApp Desktop Client
"#;
    println!("{}", banner.bright_green());
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    print_banner();

    tracing_subscriber::fmt()
        .pretty()
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .from_env_lossy()
                .add_directive("tina_slint=info".parse().unwrap())
                .add_directive("tina_worker=info".parse().unwrap())
                .add_directive("tina_ipc=info".parse().unwrap())
                .add_directive("tina_db=info".parse().unwrap()),
        )
        .init();

    info!("Starting Tina");

    // For development, use the nanachi directory in the repo
    // In production, this would be bundled or installed elsewhere
    let nanachi_dir = std::env::var("TINA_NANACHI_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            // Default to repo's nanachi directory (development mode)
            let manifest_dir = env!("CARGO_MANIFEST_DIR");
            PathBuf::from(manifest_dir).parent().unwrap().parent().unwrap().join("nanachi")
        });

    info!("Nanachi directory: {:?}", nanachi_dir);

    let ui = TinaApp::new().wrap_err("Failed to create Slint UI")?;

    let (command_tx, command_rx) = create_command_channel();
    let app_state = create_app_state();
    let ui_bridge = UiBridge::new(ui.as_weak());

    setup_ui_callbacks(&ui, command_tx.clone());

    let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .wrap_err("Failed to create Tokio runtime")?;

    let event_loop_state = app_state.clone();
    let event_loop_bridge = ui_bridge.clone();
    let event_loop_nanachi_dir = nanachi_dir.clone();

    std::thread::spawn(move || {
        tokio_runtime.block_on(async {
            match EventLoop::new(
                event_loop_nanachi_dir,
                event_loop_state,
                command_rx,
                event_loop_bridge,
            ).await {
                Ok(event_loop) => {
                    if let Err(e) = event_loop.run().await {
                        tracing::error!(?e, "Event loop error");
                    }
                }
                Err(e) => {
                    tracing::error!(?e, "Failed to create event loop");
                }
            }
        });
    });

    ui.run().wrap_err("Failed to run Slint UI")?;

    let _ = command_tx.blocking_send(Command::Shutdown);

    info!("Tina shutting down");
    Ok(())
}

fn setup_ui_callbacks(ui: &TinaApp, command_tx: CommandSender) {
    let ui_weak = ui.as_weak();
    
    let tx = command_tx.clone();
    let weak = ui_weak.clone();
    ui.global::<SlintAppState>().on_send_message(move |content| {
        let tx = tx.clone();
        let weak = weak.clone();
        let content = content.to_string();
        slint::spawn_local(async move {
            if let Some(ui) = weak.upgrade() {
                let app_state = ui.global::<SlintAppState>();
                let account_id = app_state.get_current_account_id();
                let chat_jid = app_state.get_current_chat_jid();
                
                if !account_id.is_empty() && !chat_jid.is_empty() {
                    let _ = tx.send(Command::SendMessage {
                        account_id: account_id.to_string(),
                        to: chat_jid.to_string(),
                        content,
                    }).await;
                }
            }
        }).ok();
    });

    let tx = command_tx.clone();
    ui.global::<SlintAppState>().on_select_chat(move |chat_jid| {
        let tx = tx.clone();
        let chat_jid = chat_jid.to_string();
        slint::spawn_local(async move {
            let _ = tx.send(Command::SelectChat { chat_jid }).await;
        }).ok();
    });

    let tx = command_tx.clone();
    ui.global::<SlintAppState>().on_select_account(move |account_id| {
        let tx = tx.clone();
        let account_id = account_id.to_string();
        slint::spawn_local(async move {
            let _ = tx.send(Command::SelectAccount { account_id }).await;
        }).ok();
    });

    let tx = command_tx.clone();
    ui.global::<SlintAppState>().on_create_account(move |id, name| {
        let tx = tx.clone();
        let id = if id.is_empty() {
            format!("account-{}", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs())
        } else {
            id.to_string()
        };
        let name = if name.is_empty() { id.clone() } else { name.to_string() };
        slint::spawn_local(async move {
            let _ = tx.send(Command::CreateAccount { id, name }).await;
        }).ok();
    });

    let tx = command_tx.clone();
    ui.global::<SlintAppState>().on_start_account(move |account_id| {
        let tx = tx.clone();
        let account_id = account_id.to_string();
        slint::spawn_local(async move {
            let _ = tx.send(Command::StartAccount { account_id }).await;
        }).ok();
    });

    let tx = command_tx.clone();
    ui.global::<SlintAppState>().on_stop_account(move |account_id| {
        let tx = tx.clone();
        let account_id = account_id.to_string();
        slint::spawn_local(async move {
            let _ = tx.send(Command::StopAccount { account_id }).await;
        }).ok();
    });

    let tx = command_tx.clone();
    ui.global::<SlintAppState>().on_refresh_chats(move || {
        let tx = tx.clone();
        slint::spawn_local(async move {
            let _ = tx.send(Command::RefreshChats).await;
        }).ok();
    });

    let tx = command_tx.clone();
    let weak = ui_weak.clone();
    ui.global::<SlintAppState>().on_load_messages(move |chat_jid| {
        let tx = tx.clone();
        let weak = weak.clone();
        let chat_jid = chat_jid.to_string();
        slint::spawn_local(async move {
            if let Some(ui) = weak.upgrade() {
                let account_id = ui.global::<SlintAppState>().get_current_account_id();
                if !account_id.is_empty() {
                    let _ = tx.send(Command::LoadMessages { 
                        account_id: account_id.to_string(), 
                        chat_jid 
                    }).await;
                }
            }
        }).ok();
    });
}
