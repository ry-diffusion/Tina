// Menu commands. One async fn per option — each prompts via `read_line`
// and prints the result.

use color_eyre::eyre::Result;

use tina_worker::TinaWorker;

use crate::io::read_line;

pub async fn create_account(worker: &TinaWorker) -> Result<()> {
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

pub async fn list_accounts(worker: &TinaWorker) -> Result<()> {
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

pub async fn login_account(worker: &TinaWorker) -> Result<()> {
    let id = read_line("Account ID to login: ")?;
    worker.start_account(id.trim()).await?;
    println!("Starting account {}... watch for QR code", id.trim());
    Ok(())
}

pub async fn list_contacts(_worker: &TinaWorker) -> Result<()> {
    println!("(list_contacts não exposto na CLI ainda — use list_chats)");
    Ok(())
}

pub async fn list_chats(worker: &TinaWorker) -> Result<()> {
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

pub async fn list_messages(worker: &TinaWorker) -> Result<()> {
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

pub async fn reconcile_account(worker: &TinaWorker) -> Result<()> {
    let id = read_line("Account ID: ")?;
    worker.reconcile_account(id.trim()).await?;
    println!(
        "Reconcile requested for {}. Watch the worker logs for upserts.",
        id.trim()
    );
    Ok(())
}

pub async fn send_message(worker: &TinaWorker) -> Result<()> {
    let account_id = read_line("Account ID: ")?;
    let to = read_line("To (JID): ")?;
    let content = read_line("Message: ")?;

    worker
        .send_message(account_id.trim(), to.trim(), content.trim(), &[])
        .await?;
    println!("Message sent!");
    Ok(())
}
