mod ble;
mod history;
mod protocol;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Manager;

// Флаг, чтобы случайно не запустить несколько циклов сканирования
struct BleStarter(AtomicBool);

#[tauri::command]
fn snapshot(state: tauri::State<ble::Shared>) -> ble::Snapshot {
    state.lock().unwrap().clone()
}

#[tauri::command]
fn history(state: tauri::State<ble::SharedHistory>) -> Vec<history::Sample> {
    state.lock().unwrap().all()
}

// Новая команда: запускает BLE-поток по сигналу от фронтенда
#[tauri::command]
fn start_ble(
    app: tauri::AppHandle,
    shared: tauri::State<ble::Shared>,
    history: tauri::State<ble::SharedHistory>,
    starter: tauri::State<BleStarter>,
) {
    // Гарантируем, что цикл запустится только один раз
    if !starter.0.swap(true, Ordering::SeqCst) {
        let shared_clone = shared.inner().clone();
        let history_clone = history.inner().clone();

        tauri::async_runtime::spawn(ble::run(app, shared_clone, history_clone));
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(ble::Shared::default())
        .manage(BleStarter(AtomicBool::new(false))) // Регистрируем флаг запуска
        .setup(|app| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);

            let file = app.path().app_data_dir()?.join("history.jsonl");
            let history: ble::SharedHistory =
                Arc::new(Mutex::new(history::History::load(file, now)));
            app.manage(history.clone());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![snapshot, history, start_ble]) // Добавили start_ble
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
