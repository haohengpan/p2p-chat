// Prevents additional console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Log to a known location: next to the executable
    let log_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("p2p-chat.log")))
        .unwrap_or_else(|| std::path::PathBuf::from("p2p-chat.log"));

    if let Ok(log_file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        tracing_subscriber::fmt()
            .with_writer(log_file)
            .with_ansi(false)
            .init();
        tracing::info!("=== P2P Chat started, log: {:?} ===", log_path);
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            p2p_chat::commands::setup,
            p2p_chat::commands::connect,
            p2p_chat::commands::disconnect,
            p2p_chat::commands::send_message,
            p2p_chat::commands::get_history,
            p2p_chat::commands::list_peers,
            p2p_chat::commands::shutdown,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
