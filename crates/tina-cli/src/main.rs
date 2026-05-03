use std::io::{self, Write};
use std::path::PathBuf;

use color_eyre::eyre::{Context, Result};
use tina_worker::{TinaWorker, WorkerEvent};

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

    let nanachi_dir = find_nanachi_dir()?;

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
            handle_event(event);
        }
    });

    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    loop {
        print_menu();
        let choice = read_line("Choice: ")?;

        match choice.trim() {
            "1" => create_account(&worker).await?,
            "2" => list_accounts(&worker).await?,
            "3" => login_account(&worker).await?,
            "4" => list_contacts(&worker).await?,
            "5" => list_messages(&worker).await?,
            "6" => list_chats(&worker).await?,
            "7" => send_message(&worker).await?,
            "8" => reconcile_account(&worker).await?,
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

fn print_menu() {
    println!();
    println!("╔════════════════════════════════════╗");
    println!("║          TINA CLI - WhatsApp       ║");
    println!("╠════════════════════════════════════╣");
    println!("║  1. Create Account                 ║");
    println!("║  2. List Accounts                  ║");
    println!("║  3. Login (Start Account)          ║");
    println!("║  4. List Contacts                  ║");
    println!("║  5. List Messages                  ║");
    println!("║  6. List Chats                     ║");
    println!("║  7. Send Message                   ║");
    println!("║  8. Reconcile (whatsmeow → tina)   ║");
    println!("║  0. Exit                           ║");
    println!("╚════════════════════════════════════╝");
}

fn handle_event(event: WorkerEvent) {
    match event {
        WorkerEvent::NanachiReady => {
            println!("\nNanachi is ready!");
        }
        WorkerEvent::AccountReady { account_id } => {
            println!("\nAccount {} is ready", account_id);
        }
        WorkerEvent::QrCode { account_id, qr } => {
            println!("\nQR Code for account {}:", account_id);
            print_qr_code(&qr);
        }
        WorkerEvent::Connected {
            account_id,
            phone_number,
            ..
        } => {
            println!(
                "\nConnected: {} (phone: {})",
                account_id,
                phone_number.unwrap_or_default()
            );
        }
        WorkerEvent::Disconnected { account_id, reason } => {
            println!("\nDisconnected: {} - {}", account_id, reason);
        }
        WorkerEvent::LoggedOut { account_id } => {
            println!("\nLogged out: {}", account_id);
        }
        WorkerEvent::ChatsUpserted { account_id, rows } => {
            println!("\n💬 {} chat(s) upserted for {}", rows.len(), account_id);
            for row in rows.iter().take(5) {
                println!(
                    "  · {} ({}): {}",
                    row.name,
                    row.kind,
                    row.last_message_preview.as_deref().unwrap_or("")
                );
            }
        }
        WorkerEvent::MessagesAppended {
            account_id,
            chat_id,
            messages,
        } => {
            println!(
                "\n📩 {} new message(s) in {} for {}",
                messages.len(),
                chat_id,
                account_id
            );
        }
        WorkerEvent::HistorySyncComplete {
            account_id,
            messages_count,
        } => {
            println!(
                "\nHistory sync complete for {}: {} messages",
                account_id, messages_count
            );
        }
        WorkerEvent::ReconcileProgress {
            account_id: _,
            stage,
            current,
            total,
            indeterminate: _,
        } => {
            if total > 0 {
                println!("\n🔧 {} ({}/{})", stage, current, total);
            } else {
                println!("\n🔧 {}", stage);
            }
        }
        WorkerEvent::Error { account_id, error } => {
            println!("\nError ({}): {}", account_id.unwrap_or_default(), error);
        }
        WorkerEvent::MediaDownloadProgress {
            account_id,
            message_id,
            current,
            total,
        } => {
            println!(
                "\n📥 Downloading media for {} (message {}): {}/{} bytes",
                account_id, message_id, current, total
            );
        }
        WorkerEvent::MediaReady {
            account_id,
            affected_message_ids,
            path,
            mimetype,
        } => {
            println!(
                "\n📥 Media ready for {} (messages {:?}): {} (mimetype: {})",
                account_id, affected_message_ids, path, mimetype
            );
        }
        WorkerEvent::MediaDownloadFailed {
            account_id,
            message_id,
            error,
        } => {
            println!(
                "\n❌ Failed to download media for {} (message {}): {}",
                account_id, message_id, error
            );
        }
        WorkerEvent::AvatarReady {
            account_id,
            jid,
            path,
        } => {
            println!("\n👤 Avatar ready for {} ({}): {}", account_id, jid, path);
        }
        WorkerEvent::AvatarFailed {
            account_id,
            jid,
            error,
        } => {
            println!(
                "\n❌ Failed to get avatar for {} ({}): {}",
                account_id, jid, error
            );
        }
    }
}

fn print_qr_code(qr: &str) {
    if let Err(e) = qr2term::print_qr(qr) {
        eprintln!("Failed to print QR code: {}", e);
        println!("Raw QR data: {}", qr);
    }
}

async fn create_account(worker: &TinaWorker) -> Result<()> {
    let id_input = read_line("Account ID (vazio = auto-gera UUIDv7): ")?;
    let id = if id_input.trim().is_empty() {
        uuid::Uuid::now_v7().to_string()
    } else {
        id_input.trim().to_string()
    };
    let name = read_line("Account Name (optional): ")?;
    let name_opt = if name.trim().is_empty() {
        None
    } else {
        Some(name.trim())
    };

    let account = worker.create_account(&id, name_opt).await?;
    println!(
        "Created account: {} ({})",
        account.id,
        account.name.unwrap_or_default()
    );
    Ok(())
}

async fn list_accounts(worker: &TinaWorker) -> Result<()> {
    let accounts = worker.list_accounts().await?;

    if accounts.is_empty() {
        println!("No accounts found");
    } else {
        println!("\nAccounts:");
        for account in accounts {
            let has_auth = if account.phone_number.is_some() {
                "[AUTH]"
            } else {
                "[NO AUTH]"
            };
            println!(
                "  {} {} - {} {}",
                has_auth,
                account.id,
                account.name.unwrap_or_default(),
                account.phone_number.unwrap_or_default()
            );
        }
    }
    Ok(())
}

async fn login_account(worker: &TinaWorker) -> Result<()> {
    let id = read_line("Account ID to login: ")?;
    worker.start_account(id.trim()).await?;
    println!("Starting account {}... watch for QR code", id.trim());
    Ok(())
}

async fn list_contacts(_worker: &TinaWorker) -> Result<()> {
    println!("(list_contacts não exposto na CLI ainda — use list_chats)");
    Ok(())
}

async fn list_chats(worker: &TinaWorker) -> Result<()> {
    let id = read_line("Account ID: ")?;
    let rows = worker.list_chat_rows(id.trim()).await?;

    if rows.is_empty() {
        println!("No chats found");
    } else {
        println!("\nChats ({}):", rows.len());
        for (i, row) in rows.iter().enumerate().take(40) {
            println!(
                "  {:>3}. [{:<10}] {} — {}",
                i + 1,
                row.kind,
                row.name,
                row.last_message_preview.as_deref().unwrap_or("")
            );
        }
        if rows.len() > 40 {
            println!("  ... and {} more", rows.len() - 40);
        }
    }
    Ok(())
}

async fn list_messages(worker: &TinaWorker) -> Result<()> {
    let account_id = read_line("Account ID: ")?;
    let chat_id = read_line("Chat ID: ")?;

    let messages = worker
        .get_messages(account_id.trim(), chat_id.trim(), 20, 0)
        .await?;

    if messages.is_empty() {
        println!("No messages found");
    } else {
        println!("\nMessages ({}):", messages.len());
        for msg in messages {
            let direction = if msg.is_from_me { "→" } else { "←" };
            println!(
                "  {} [{}] {}: {}",
                direction,
                msg.message_type,
                msg.sender_contact_id.as_deref().unwrap_or("?"),
                msg.content.as_deref().unwrap_or("[media]")
            );
        }
    }
    Ok(())
}

async fn reconcile_account(worker: &TinaWorker) -> Result<()> {
    let id = read_line("Account ID: ")?;
    worker.reconcile_account(id.trim()).await?;
    println!(
        "Reconcile requested for {}. Watch the worker logs for upserts.",
        id.trim()
    );
    Ok(())
}

async fn send_message(worker: &TinaWorker) -> Result<()> {
    let account_id = read_line("Account ID: ")?;
    let to = read_line("To (JID): ")?;
    let content = read_line("Message: ")?;

    worker
        .send_message(account_id.trim(), to.trim(), content.trim())
        .await?;
    println!("Message sent!");
    Ok(())
}

fn read_line(prompt: &str) -> Result<String> {
    print!("{}", prompt);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

fn find_nanachi_dir() -> Result<PathBuf> {
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
        "Could not find nanachi directory. Make sure you're running from the project root."
    ))
}
