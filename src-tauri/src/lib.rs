mod ports;

use ports::PortInfo;
use std::path::{Path, PathBuf};
use tauri::Manager;

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

/// 探测目录是否真的可写（能建目录不代表 WebView2 能写文件）
fn is_writable(dir: &Path) -> std::io::Result<()> {
    let probe = dir.join(".killports-write-test");
    std::fs::write(&probe, b"")?;
    let _ = std::fs::remove_file(probe);
    Ok(())
}

/// 准备 WebView2 用户数据目录。
///
/// Tauri 默认把数据目录设为 `%LOCALAPPDATA%\<identifier>`，且会在窗口创建前
/// `create_dir_all` 它——目录建不出来就直接 `tauri::Error::Io` panic
/// （表现为 `Failed to setup app: 拒绝访问。 (os error 5)`）。
/// 这里依次探测候选目录，取第一个可写的，避免受限账号 / 沙箱 / 组策略下无法启动。
fn resolve_webview_data_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    match app.path().app_local_data_dir() {
        Ok(dir) => candidates.push(dir),
        Err(e) => eprintln!("[killports] 无法解析应用数据目录: {e}"),
    }
    // 兜底：系统临时目录（受限环境下通常仍可写）
    candidates.push(std::env::temp_dir().join("KillPorts"));

    let mut errors: Vec<String> = Vec::new();
    let first = candidates.first().cloned();

    for dir in &candidates {
        match std::fs::create_dir_all(dir).and_then(|_| is_writable(dir)) {
            Ok(()) => {
                if Some(dir) != first.as_ref() {
                    eprintln!(
                        "[killports] 默认数据目录不可写，已回退到: {}",
                        dir.display()
                    );
                }
                return Ok(dir.clone());
            }
            Err(e) => errors.push(format!("{} -> {e}", dir.display())),
        }
    }

    Err(format!(
        "无法创建 WebView2 用户数据目录，应用无法启动：\n  {}",
        errors.join("\n  ")
    ))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // 窗口由这里手动创建（tauri.conf.json 里 create: false），
            // 以便在创建前接管 WebView2 数据目录，并保留配置文件里的窗口参数。
            let window_config = app
                .config()
                .app
                .windows
                .first()
                .cloned()
                .ok_or("tauri.conf.json 中缺少窗口配置")?;

            let data_dir = resolve_webview_data_dir(app.handle())?;

            tauri::WebviewWindowBuilder::from_config(app.handle(), &window_config)?
                .data_directory(data_dir)
                .build()?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_listening_ports,
            kill_process,
            is_elevated,
            relaunch_as_admin
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
