// Prevents additional console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use p2p_chat::storage::Storage;

fn main() {
    // Determine app data directory:
    //   Windows: %APPDATA%/com.p2pchat.app/
    //   Linux:   ~/.config/com.p2pchat.app/
    let data_dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("com.p2pchat.app");

    let storage = Storage::new(data_dir);
    if let Err(e) = storage.init() {
        eprintln!("Failed to init storage: {}", e);
    }

    // File-based logging in app data dir
    if let Ok(log_file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(storage.log_path())
    {
        tracing_subscriber::fmt()
            .with_writer(log_file)
            .with_ansi(false)
            .init();
        tracing::info!("=== P2P Chat started, data dir: {:?} ===", storage.data_dir());
    }

    // Make storage globally accessible
    p2p_chat::bridge::init_storage(storage);

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            p2p_chat::commands::list_profiles,
            p2p_chat::commands::get_saved_peers,
            p2p_chat::commands::load_history,
            p2p_chat::commands::setup,
            p2p_chat::commands::login,
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
