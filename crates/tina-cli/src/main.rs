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
    println!("📁 Nanachi directory: {}", nanachi_dir.display());

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
            "0" => {
                println!("👋 Shutting down...");
                worker.stop().await?;
                break;
            }
            _ => println!("❌ Invalid choice"),
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
    println!("║  0. Exit                           ║");
    println!("╚════════════════════════════════════╝");
}

fn handle_event(event: WorkerEvent) {
    match event {
        WorkerEvent::NanachiReady => {
            println!("\n🚀 Nanachi is ready!");
        }
        WorkerEvent::AccountReady { account_id } => {
            println!("\n✅ Account {} is ready", account_id);
        }
        WorkerEvent::QrCode { account_id, qr } => {
            println!("\n📱 QR Code for account {}:", account_id);
            print_qr_code(&qr);
        }
        WorkerEvent::Connected { account_id, phone_number } => {
            println!(
                "\n✅ Connected: {} (phone: {})",
                account_id,
                phone_number.unwrap_or_default()
            );
        }
        WorkerEvent::Disconnected { account_id, reason } => {
            println!("\n❌ Disconnected: {} - {}", account_id, reason);
        }
        WorkerEvent::LoggedOut { account_id } => {
            println!("\n🚪 Logged out: {}", account_id);
        }
        WorkerEvent::ContactsSynced { account_id, count } => {
            println!("\n📇 Synced {} contacts for {}", count, account_id);
        }
        WorkerEvent::GroupsSynced { account_id, count } => {
            println!("\n👥 Synced {} groups for {}", count, account_id);
        }
        WorkerEvent::MessagesSynced { account_id, count } => {
            println!("\n💬 Synced {} messages for {}", count, account_id);
        }
        WorkerEvent::HistorySyncComplete {
            account_id,
            messages_count,
        } => {
            println!(
                "\n📜 History sync complete for {}: {} messages",
                account_id, messages_count
            );
        }
        WorkerEvent::Error { account_id, error } => {
            println!(
                "\n❌ Error ({}): {}",
                account_id.unwrap_or_default(),
                error
            );
        }
        WorkerEvent::SyncStarted { account_id, sync_type } => {
            println!("\n⏳ Sync started for {}: {}", account_id, sync_type);
        }
        WorkerEvent::SyncProgress { account_id: _, sync_type, current, total } => {
            if let Some(t) = total {
                println!("   {} sync progress: {}/{}", sync_type, current, t);
            } else {
                println!("   {} sync progress: {}...", sync_type, current);
            }
        }
        WorkerEvent::SyncCompleted { account_id, sync_type, count } => {
            println!("\n✅ Sync completed for {}: {} ({} items)", account_id, sync_type, count);
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
    let id = read_line("Account ID: ")?;
    let name = read_line("Account Name (optional): ")?;
    let name_opt = if name.trim().is_empty() {
        None
    } else {
        Some(name.trim())
    };

    let account = worker.create_account(id.trim(), name_opt).await?;
    println!("✅ Created account: {} ({})", account.id, account.name.unwrap_or_default());
    Ok(())
}

async fn list_accounts(worker: &TinaWorker) -> Result<()> {
    let accounts = worker.list_accounts().await?;

    if accounts.is_empty() {
        println!("📭 No accounts found");
    } else {
        println!("\n📋 Accounts:");
        for account in accounts {
            let has_auth = if account.auth_state.is_some() {
                "🔑"
            } else {
                "❌"
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
    println!("🔄 Starting account {}... watch for QR code", id.trim());
    Ok(())
}

async fn list_contacts(worker: &TinaWorker) -> Result<()> {
    let id = read_line("Account ID: ")?;
    let contacts = worker.get_contacts(id.trim()).await?;

    if contacts.is_empty() {
        println!("📭 No contacts found");
    } else {
        println!("\n📇 Contacts ({}):", contacts.len());
        for contact in contacts.iter().take(20) {
            println!(
                "  📱 {} - {} ({})",
                contact.jid,
                contact.name.as_deref().unwrap_or("?"),
                contact.phone_number.as_deref().unwrap_or("?")
            );
        }
        if contacts.len() > 20 {
            println!("  ... and {} more", contacts.len() - 20);
        }
    }
    Ok(())
}

async fn list_chats(worker: &TinaWorker) -> Result<()> {
    let id = read_line("Account ID: ")?;
    let chats = worker.get_chats(id.trim()).await?;

    if chats.is_empty() {
        println!("📭 No chats found");
    } else {
        println!("\n💬 Chats ({}):", chats.len());
        for (i, chat) in chats.iter().enumerate().take(20) {
            println!("  {}. {}", i + 1, chat);
        }
        if chats.len() > 20 {
            println!("  ... and {} more", chats.len() - 20);
        }
    }
    Ok(())
}

async fn list_messages(worker: &TinaWorker) -> Result<()> {
    let account_id = read_line("Account ID: ")?;
    let chat_jid = read_line("Chat JID (empty for all): ")?;

    let chat_opt = if chat_jid.trim().is_empty() {
        None
    } else {
        Some(chat_jid.trim())
    };

    let messages = worker
        .get_messages(account_id.trim(), chat_opt, 20, 0)
        .await?;

    if messages.is_empty() {
        println!("📭 No messages found");
    } else {
        println!("\n💬 Messages ({}):", messages.len());
        for msg in messages {
            let direction = if msg.is_from_me { "→" } else { "←" };
            println!(
                "  {} [{}] {}: {}",
                direction,
                msg.message_type,
                msg.sender_jid,
                msg.content.as_deref().unwrap_or("[media]")
            );
        }
    }
    Ok(())
}

async fn send_message(worker: &TinaWorker) -> Result<()> {
    let account_id = read_line("Account ID: ")?;
    let to = read_line("To (JID): ")?;
    let content = read_line("Message: ")?;

    worker
        .send_message(account_id.trim(), to.trim(), content.trim())
        .await?;
    println!("📤 Message sent!");
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
