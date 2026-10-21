use netstat2::{get_sockets_info, AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo, TcpState};
use serde::Serialize;
use std::collections::HashMap;
use sysinfo::{ProcessesToUpdate, Pid, System};

/// 一条监听端口记录
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PortInfo {
    /// TCP / UDP
    pub protocol: String,
    /// 本地监听地址，如 0.0.0.0、127.0.0.1、[::]
    pub local_addr: String,
    pub port: u16,
    /// LISTENING / BOUND
    pub state: String,
    pub pid: Option<u32>,
    pub process_name: Option<String>,
    pub exe_path: Option<String>,
    pub command_line: Option<String>,
    /// 常驻内存（字节）
    pub memory: Option<u64>,
    /// 是否系统进程（Windows 系统目录下的可执行文件，或内核组件）
    pub system: bool,
}

/// 归一化路径：统一分隔符、去掉 Windows 扩展长度前缀（\\?\ 与 \\?\UNC\）、转小写
fn normalize_path(path: &str) -> String {
    let lower = path.replace('/', "\\").to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix(r"\\?\unc\") {
        return format!("\\\\{rest}");
    }
    if let Some(rest) = lower.strip_prefix(r"\\?\") {
        return rest.to_string();
    }
    lower
}

/// 路径是否位于指定目录之下（大小写与分隔符不敏感，兼容扩展长度前缀）
fn is_under_dir(path: &str, dir: &str) -> bool {
    let p = normalize_path(path);
    let d = normalize_path(dir);
    let d = d.trim_end_matches('\\');
    if d.is_empty() {
        return false;
    }
    p == d || p.starts_with(&format!("{d}\\"))
}

/// 系统目录清单：Windows 下取 %SystemRoot% / %windir%
fn system_dirs() -> Vec<String> {
    let mut dirs: Vec<String> = Vec::new();
    for key in ["SystemRoot", "windir"] {
        if let Ok(v) = std::env::var(key) {
            let v = v.trim().to_string();
            if !v.is_empty() && !dirs.iter().any(|d| d.eq_ignore_ascii_case(&v)) {
                dirs.push(v);
            }
        }
    }
    dirs
}

/// 没有 exe 路径时，仅凭进程名即可识别的内核 / 系统组件
const SYSTEM_PROCESS_NAMES: &[&str] = &[
    "system",
    "system idle process",
    "registry",
    "memory compression",
    "secure system",
    "svchost.exe",
    "lsass.exe",
    "services.exe",
    "wininit.exe",
    "winlogon.exe",
    "csrss.exe",
    "smss.exe",
    "spoolsv.exe",
    "fontdrvhost.exe",
    "dwm.exe",
];

/// PID 0（System Idle）与 PID 4（System / Windows 内核）本身即系统组件
fn is_system_pid(pid: Option<u32>) -> bool {
    matches!(pid, Some(0) | Some(4))
}

/// 判断是否为系统进程：可执行文件位于系统目录，或缺少路径时命中内核组件名。
/// 若给出了明确路径且不在系统目录（例如用户目录下同名的 svchost.exe），按普通用户程序处理。
fn is_system_process_in(name: Option<&str>, exe: Option<&str>, dirs: &[String]) -> bool {
    if let Some(path) = exe {
        if dirs.iter().any(|d| is_under_dir(path, d)) {
            return true;
        }
    }
    match name {
        Some(n) => SYSTEM_PROCESS_NAMES.contains(&n.trim().to_ascii_lowercase().as_str()) && exe.is_none(),
        None => false,
    }
}

/// 枚举本机所有监听中的端口（TCP LISTEN + UDP 已绑定）
pub fn list_listening_ports() -> Result<Vec<PortInfo>, String> {
    let af_flags = AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6;
    let proto_flags = ProtocolFlags::TCP | ProtocolFlags::UDP;

    let sockets_info =
        get_sockets_info(af_flags, proto_flags).map_err(|e| format!("枚举端口失败: {e}"))?;

    // 只刷新进程列表，拿 PID -> 进程名/路径 映射
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);

    let mut cache: HashMap<u32, Option<(String, Option<String>, String, u64)>> = HashMap::new();
    let mut lookup = |pid: u32| -> Option<(String, Option<String>, String, u64)> {
        if let Some(hit) = cache.get(&pid) {
            return hit.clone();
        }
        let info = sys.process(Pid::from_u32(pid)).map(|p| {
            let cmd = p
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ");
            (
                p.name().to_string_lossy().trim().to_string(),
                p.exe().map(|e| e.display().to_string()),
                cmd,
                p.memory(),
            )
        });
        cache.insert(pid, info.clone());
        info
    };

    let sys_dirs = system_dirs();
    let mut out: Vec<PortInfo> = Vec::new();
    for si in sockets_info {
        let (protocol, local_addr, port, state) = match &si.protocol_socket_info {
            ProtocolSocketInfo::Tcp(tcp) => {
                if !matches!(tcp.state, TcpState::Listen) {
                    continue;
                }
                ("TCP", tcp.local_addr.to_string(), tcp.local_port, "LISTENING")
            }
            ProtocolSocketInfo::Udp(udp) => {
                ("UDP", udp.local_addr.to_string(), udp.local_port, "BOUND")
            }
        };

        let pid = si.associated_pids.first().copied();
        let (process_name, exe_path, cmd, memory) = match pid {
            Some(p) => lookup(p)
                .map(|(n, e, c, m)| (Some(n), e, Some(c), Some(m)))
                .unwrap_or((None, None, None, None)),
            None => (None, None, None, None),
        };
        let system = is_system_pid(pid)
            || is_system_process_in(process_name.as_deref(), exe_path.as_deref(), &sys_dirs);

        out.push(PortInfo {
            protocol: protocol.to_string(),
            local_addr,
            port,
            state: state.to_string(),
            pid,
            process_name,
            exe_path,
            command_line: cmd.filter(|c| !c.is_empty()),
            memory,
            system,
        });
    }

    out.sort_by(|a, b| {
        a.port
            .cmp(&b.port)
            .then_with(|| a.protocol.cmp(&b.protocol))
            .then_with(|| a.local_addr.cmp(&b.local_addr))
    });
    Ok(out)
}

/// 结束指定 PID 的进程（系统级强杀）
pub fn kill_process(pid: u32) -> Result<(), String> {
    // 自我保护：不允许结束自己 / 空闲进程 / 系统内核
    if pid == std::process::id() {
        return Err("不能结束本应用自身的进程".into());
    }
    if pid == 0 || pid == 4 {
        return Err("PID {pid} 是系统内核进程，禁止结束".replace("{pid}", &pid.to_string()));
    }

    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let proc = sys
        .process(Pid::from_u32(pid))
        .ok_or_else(|| format!("进程 {pid} 不存在或已经退出"))?;

    let name = proc.name().to_string_lossy().to_string();
    if proc.kill() {
        Ok(())
    } else {
        Err(format!(
            "结束进程 {pid} ({name}) 失败：权限不足或进程受系统保护。请尝试以管理员身份运行本应用。"
        ))
    }
}

#[cfg(windows)]
pub fn is_elevated() -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation: TOKEN_ELEVATION = std::mem::zeroed();
        let mut ret_len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    // 非 Windows 平台交给 sudo 管理权限
    true
}

/// 以管理员身份重启应用（仅 Windows）
#[cfg(windows)]
pub fn relaunch_as_admin(app: &tauri::AppHandle) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let exe = std::env::current_exe().map_err(|e| format!("获取程序路径失败: {e}"))?;
    let script = format!("Start-Process -FilePath '{}' -Verb RunAs", exe.display());
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("提权重启失败: {e}"))?;
    app.exit(0);
    Ok(())
}

#[cfg(not(windows))]
pub fn relaunch_as_admin(_app: &tauri::AppHandle) -> Result<(), String> {
    Err("当前平台暂不支持自动提权重启".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_system_process_classification() {
        let dirs = vec!["C:\\Windows".to_string()];

        // 系统目录下的可执行文件 → 系统进程（大小写 / 分隔符不敏感）
        assert!(is_system_process_in(
            Some("svchost.exe"),
            Some("C:\\Windows\\System32\\svchost.exe"),
            &dirs
        ));
        assert!(is_system_process_in(
            Some("explorer.exe"),
            Some("c:/windows/explorer.exe"),
            &dirs
        ));
        assert!(is_system_process_in(
            Some("SearchIndexer.exe"),
            Some("C:\\Windows\\System32\\SearchIndexer.exe"),
            &dirs
        ));

        // 内核 / 系统组件通常拿不到 exe 路径，按进程名识别
        assert!(is_system_process_in(Some("System"), None, &dirs));
        assert!(is_system_process_in(Some("Registry"), None, &dirs));
        assert!(is_system_process_in(Some("svchost.exe"), None, &dirs));

        // 用户程序：路径不在系统目录
        assert!(!is_system_process_in(
            Some("node.exe"),
            Some("C:\\Program Files\\nodejs\\node.exe"),
            &dirs
        ));
        assert!(!is_system_process_in(
            Some("java.exe"),
            Some("D:\\dev\\jdk\\bin\\java.exe"),
            &dirs
        ));

        // 同名但位于用户目录 → 按普通程序处理
        assert!(!is_system_process_in(
            Some("svchost.exe"),
            Some("D:\\tools\\svchost.exe"),
            &dirs
        ));

        // 未知进程名且没有路径
        assert!(!is_system_process_in(Some("myapp.exe"), None, &dirs));
        assert!(!is_system_process_in(None, None, &dirs));

        // 空目录清单不应把所有路径都判成系统进程
        assert!(!is_system_process_in(Some("node.exe"), Some("C:\\x\\node.exe"), &[]));

        // PID 0 / 4 本身就是内核组件
        assert!(is_system_pid(Some(0)));
        assert!(is_system_pid(Some(4)));
        assert!(!is_system_pid(Some(1234)));
        assert!(!is_system_pid(None));
    }

    #[test]
    fn test_list_listening_ports() {
        let ports = list_listening_ports().expect("枚举失败");
        println!("共枚举到 {} 个监听端口", ports.len());
        for p in ports.iter().take(10) {
            println!(
                "  {} {}:{}  pid={:?}  proc={:?}  system={}  exe={:?}",
                p.protocol, p.local_addr, p.port, p.pid, p.process_name, p.system, p.exe_path
            );
        }
        assert!(!ports.is_empty(), "本机至少应有一个监听端口");
        assert!(ports.iter().all(|p| p.port > 0));
        assert!(ports.is_sorted_by_key(|p| (p.port, p.protocol.clone())));

        // 系统目录下的可执行文件必须被标记为系统进程
        let dirs = system_dirs();
        for p in &ports {
            if let Some(exe) = &p.exe_path {
                if dirs.iter().any(|d| is_under_dir(exe, d)) {
                    assert!(p.system, "{exe} 位于系统目录却未标记为系统进程");
                }
            }
        }
        let sys_count = ports.iter().filter(|p| p.system).count();
        println!("其中系统进程占用 {sys_count} 条（系统目录：{dirs:?}）");
        assert!(sys_count <= ports.len());

        // 真实数据抽查：PID 4（System）占用的端口必须被标记为系统进程
        if let Some(p) = ports.iter().find(|p| p.pid == Some(4)) {
            assert!(p.system, "PID 4 占用的端口未被标记为系统进程");
        }

        // 真实数据抽查：svchost.exe 一定来自系统目录，必须被识别
        if let Some(p) = ports.iter().find(|p| {
            p.process_name
                .as_deref()
                .map(|n| n.eq_ignore_ascii_case("svchost.exe"))
                .unwrap_or(false)
        }) {
            assert!(p.system, "svchost.exe 未被识别为系统进程：{:?}", p.exe_path);
        }
    }
}
