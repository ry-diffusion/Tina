// stdin/stdout helpers + the nanachi-dir locator.

use std::io::{self, Write};
use std::path::PathBuf;

use color_eyre::eyre::Result;

pub fn read_line(prompt: &str) -> Result<String> {
    print!("{}", prompt);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

pub fn print_menu() {
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

pub fn find_nanachi_dir() -> Result<PathBuf> {
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
