// `WorkerEvent` printer: the CLI's only "UI" — translates each event
// into a human-readable line on stdout.

use tina_worker::WorkerEvent;

pub fn handle_event(event: WorkerEvent) {
    match event {
        WorkerEvent::NanachiReady => println!("\nNanachi is ready!"),
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
                "\n📥 Media ready for {} (messages {:?}): {} (mimetype: {:?})",
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
