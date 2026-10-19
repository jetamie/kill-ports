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
    fn test_list_listening_ports() {
        let ports = list_listening_ports().expect("枚举失败");
        println!("共枚举到 {} 个监听端口", ports.len());
        for p in ports.iter().take(10) {
            println!(
                "  {} {}:{}  pid={:?}  proc={:?}",
                p.protocol, p.local_addr, p.port, p.pid, p.process_name
            );
        }
        assert!(!ports.is_empty(), "本机至少应有一个监听端口");
        assert!(ports.iter().all(|p| p.port > 0));
        assert!(ports.is_sorted_by_key(|p| (p.port, p.protocol.clone())));
    }
}
