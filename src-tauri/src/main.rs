// Prevents additional console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // File-based logging (same as TUI version)
    if let Ok(log_file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("p2p-chat.log")
    {
        tracing_subscriber::fmt()
            .with_writer(log_file)
            .with_ansi(false)
            .init();
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
