mod ble;
mod history;
mod protocol;

use std::sync::{Arc, Mutex};
use tauri::Manager;

#[tauri::command]
fn snapshot(state: tauri::State<ble::Shared>) -> ble::Snapshot {
    state.lock().unwrap().clone()
}

#[tauri::command]
fn history(state: tauri::State<ble::SharedHistory>) -> Vec<history::Sample> {
    state.lock().unwrap().all()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(ble::Shared::default())
        .setup(|app| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let file = app.path().app_data_dir()?.join("history.jsonl");
            let history: ble::SharedHistory = Arc::new(Mutex::new(history::History::load(file, now)));
            app.manage(history.clone());
            let shared = app.state::<ble::Shared>().inner().clone();
            tauri::async_runtime::spawn(ble::run(app.handle().clone(), shared, history));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![snapshot, history])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
