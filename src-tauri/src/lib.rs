mod ports;

use ports::PortInfo;

#[tauri::command]
fn list_listening_ports() -> Result<Vec<PortInfo>, String> {
    ports::list_listening_ports()
}

#[tauri::command]
fn kill_process(pid: u32) -> Result<(), String> {
    ports::kill_process(pid)
}

#[tauri::command]
fn is_elevated() -> bool {
    ports::is_elevated()
}

#[tauri::command]
fn relaunch_as_admin(app: tauri::AppHandle) -> Result<(), String> {
    ports::relaunch_as_admin(&app)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            list_listening_ports,
            kill_process,
            is_elevated,
            relaunch_as_admin
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
