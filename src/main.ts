import { invoke } from "@tauri-apps/api/core";

interface PortInfo {
  protocol: string;
  localAddr: string;
  port: number;
  state: string;
  pid: number | null;
  processName: string | null;
  exePath: string | null;
  commandLine: string | null;
  memory: number | null;
}

const $ = <T extends HTMLElement = HTMLElement>(sel: string) =>
  document.querySelector(sel) as T;

const tbody = $("#tbody");
const emptyBox = $("#empty");
const emptyText = $("#empty-text");
const loading = $("#loading");
const search = $<HTMLInputElement>("#search");
const autoRefresh = $<HTMLInputElement>("#auto-refresh");
const refreshBtn = $("#btn-refresh");
const banner = $("#banner");
const bannerText = $("#banner-text");
const btnAdmin = $("#btn-admin");
const modal = $("#modal");
const statusLeft = $("#status-left");
const toasts = $("#toasts");

let all: PortInfo[] = [];
let protoFilter = "all";
let scanning = false;
let timer: number | undefined;
let pending: PortInfo | null = null;

// 在普通浏览器里运行时使用演示数据（开发预览用）
const inTauri = "__TAURI_INTERNALS__" in window;

async function fetchPorts(): Promise<PortInfo[]> {
  if (inTauri) {
    return invoke<PortInfo[]>("list_listening_ports");
  }
  await new Promise((r) => setTimeout(r, 400)); // 模拟延迟
  return mockData();
}

function fmtMem(bytes: number | null): string {
  if (bytes == null) return "–";
  const kb = bytes / 1024;
  if (kb < 1024) return `${kb.toFixed(0)} KB`;
  return `${(kb / 1024).toFixed(1)} MB`;
}

function esc(s: string | null): string {
  if (!s) return "";
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function filtered(): PortInfo[] {
  const q = search.value.trim().toLowerCase();
  return all.filter((p) => {
    if (protoFilter !== "all" && p.protocol !== protoFilter) return false;
    if (!q) return true;
    const hay = [
      String(p.port),
      p.protocol,
      p.localAddr,
      p.state,
      p.processName ?? "",
      p.pid != null ? String(p.pid) : "",
      p.exePath ?? "",
    ]
      .join(" ")
      .toLowerCase();
    return hay.includes(q);
  });
}

function render() {
  const rows = filtered();

  const tcp = all.filter((p) => p.protocol === "TCP").length;
  const udp = all.filter((p) => p.protocol === "UDP").length;
  const procs = new Set(all.map((p) => p.pid).filter((x) => x != null)).size;
  $("#stat-total").textContent = String(all.length);
  $("#stat-tcp").textContent = String(tcp);
  $("#stat-udp").textContent = String(udp);
  $("#stat-procs").textContent = String(procs);

  tbody.innerHTML = rows
    .map((p) => {
      const name = p.processName ?? "未知进程";
      const canKill = p.pid != null;
      return `
      <tr data-port="${p.port}" data-pid="${p.pid ?? ""}" data-name="${esc(name)}"
          data-exe="${esc(p.exePath ?? "")}" data-addr="${esc(p.localAddr)}" data-proto="${p.protocol}">
        <td><span class="port-num">${p.port}</span></td>
        <td><span class="badge ${p.protocol.toLowerCase()}">${p.protocol}</span></td>
        <td class="addr">${esc(p.localAddr)}<span class="port-echo">:${p.port}</span></td>
        <td><span class="state ${p.protocol === "UDP" ? "udp" : ""}"><span class="dot"></span>${esc(p.state)}</span></td>
        <td><div class="proc"><span class="proc-name ${p.processName ? "" : "unknown"}" title="${esc(p.exePath ?? name)}">${esc(name)}</span></div></td>
        <td class="pid">${p.pid ?? "–"}</td>
        <td class="mem">${fmtMem(p.memory)}</td>
        <td class="cell-act"><button class="kill-btn" data-kill="${p.pid ?? ""}" ${canKill ? "" : "disabled"} title="强制结束该进程">
          <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>结束
        </button></td>
      </tr>`;
    })
    .join("");

  emptyBox.classList.toggle("hidden", rows.length > 0);
  if (rows.length === 0) {
    emptyText.textContent = all.length
      ? "没有匹配的监听端口，换个关键词试试"
      : "当前没有监听中的端口";
  }

  statusLeft.textContent = `最后扫描 ${new Date().toLocaleTimeString("zh-CN", { hour12: false })} · 共 ${all.length} 条监听记录`;
}

async function refresh(initial = false) {
  if (scanning) return;
  scanning = true;
  if (!initial) loading.classList.remove("hidden");
  try {
    all = await fetchPorts();
    render();
  } catch (e) {
    toast(`扫描失败: ${String(e)}`, "err");
  } finally {
    scanning = false;
    loading.classList.add("hidden");
  }
}

// ---------------- kill flow ----------------
function openModal(p: PortInfo) {
  pending = p;
  $("#m-name").textContent = p.processName ?? "未知进程";
  $("#m-pid").textContent = String(p.pid ?? "–");
  $("#m-port").textContent = `${p.protocol} ${p.localAddr}:${p.port}`;
  $("#m-exe").textContent = p.exePath ?? "（无法获取可执行文件路径）";
  modal.classList.remove("hidden");
  $("#btn-kill").focus();
}

function closeModal() {
  pending = null;
  modal.classList.add("hidden");
}

async function doKill() {
  if (!pending || pending.pid == null) return;
  const pid = pending.pid;
  const name = pending.processName ?? String(pid);
  const btn = $("#btn-kill") as HTMLButtonElement;
  btn.disabled = true;
  btn.textContent = "正在结束…";
  try {
    await invoke("kill_process", { pid });
    toast(`已结束进程 ${name} (PID ${pid})`, "ok");
    closeModal();
    await refresh();
  } catch (e) {
    toast(String(e), "err");
    btn.disabled = false;
    btn.textContent = "强制结束";
  }
}

tbody.addEventListener("click", (ev) => {
  const btn = (ev.target as HTMLElement).closest("[data-kill]") as HTMLElement | null;
  if (!btn) return;
  const pid = btn.dataset.kill;
  if (!pid) return;
  const p = all.find((x) => String(x.pid) === pid);
  if (p) openModal(p);
});

tbody.addEventListener("dblclick", (ev) => {
  const tr = (ev.target as HTMLElement).closest("tr") as HTMLElement | null;
  if (!tr?.dataset.pid) return;
  const cmd = `taskkill /F /PID ${tr.dataset.pid}`;
  navigator.clipboard
    ?.writeText(cmd)
    .then(() => toast(`已复制: ${cmd}`, "ok"))
    .catch(() => toast(cmd, "ok"));
});

// ---------------- events ----------------
search.addEventListener("input", render);
$("#modal-close").addEventListener("click", closeModal);
$("#btn-cancel").addEventListener("click", closeModal);
$("#btn-kill").addEventListener("click", doKill);
modal.addEventListener("mousedown", (ev) => {
  if (ev.target === modal) closeModal();
});
document.addEventListener("keydown", (ev) => {
  if (ev.key === "Escape") closeModal();
  if ((ev.ctrlKey || ev.metaKey) && ev.key.toLowerCase() === "r") {
    ev.preventDefault();
    refresh();
  }
  if ((ev.ctrlKey || ev.metaKey) && ev.key.toLowerCase() === "f") {
    ev.preventDefault();
    search.focus();
  }
});
refreshBtn.addEventListener("click", () => refresh());
$("#tabs").addEventListener("click", (ev) => {
  const tab = (ev.target as HTMLElement).closest(".tab") as HTMLElement | null;
  if (!tab) return;
  protoFilter = tab.dataset.proto ?? "all";
  document
    .querySelectorAll(".tab")
    .forEach((t) => t.classList.toggle("active", t === tab));
  render();
});
autoRefresh.addEventListener("change", () => {
  if (autoRefresh.checked) {
    timer = window.setInterval(() => {
      if (modal.classList.contains("hidden")) refresh();
    }, 4000);
    toast("已开启自动刷新（每 4 秒）", "ok");
  } else {
    clearInterval(timer);
    toast("已关闭自动刷新");
  }
});

// ---------------- admin ----------------
if (inTauri) {
  invoke<boolean>("is_elevated")
    .then((elev) => {
      banner.classList.toggle("hidden", elev);
      if (!elev) bannerText.textContent = "未以管理员身份运行：结束系统进程可能失败。";
    })
    .catch(() => banner.classList.add("hidden"));
  btnAdmin.addEventListener("click", async () => {
    try {
      await invoke("relaunch_as_admin");
    } catch (e) {
      toast(String(e), "err");
    }
  });
} else {
  banner.classList.add("hidden");
}

// ---------------- toast ----------------
function toast(msg: string, kind: "" | "ok" | "err" = "") {
  const el = document.createElement("div");
  el.className = `toast ${kind}`;
  el.textContent = msg;
  toasts.appendChild(el);
  setTimeout(() => {
    el.style.transition = "opacity .25s, transform .25s";
    el.style.opacity = "0";
    el.style.transform = "translateY(6px)";
    setTimeout(() => el.remove(), 260);
  }, 3200);
}

// ---------------- mock (browser preview only) ----------------
function mockData(): PortInfo[] {
  const mk = (
    port: number,
    proto: "TCP" | "UDP",
    addr: string,
    pid: number | null,
    name: string | null,
    mem: number | null,
  ): PortInfo => ({
    protocol: proto,
    localAddr: addr,
    port,
    state: proto === "TCP" ? "LISTENING" : "BOUND",
    pid,
    processName: name,
    exePath: name ? `C:\\Program Files\\Demo\\${name}` : null,
    commandLine: null,
    memory: mem,
  });
  return [
    mk(135, "TCP", "0.0.0.0", 512, "svchost.exe", 12 * 1024 * 1024),
    mk(445, "TCP", "0.0.0.0", 4, null, null),
    mk(5040, "TCP", "127.0.0.1", 3804, "svchost.exe", 8 * 1024 * 1024),
    mk(3389, "TCP", "0.0.0.0", 1424, "svchost.exe", 15 * 1024 * 1024),
    mk(8080, "TCP", "0.0.0.0", 9120, "java.exe", 512 * 1024 * 1024),
    mk(3000, "TCP", "127.0.0.1", 15420, "node.exe", 96 * 1024 * 1024),
    mk(5173, "TCP", "0.0.0.0", 17364, "node.exe", 84 * 1024 * 1024),
    mk(3306, "TCP", "0.0.0.0", 6412, "mysqld.exe", 480 * 1024 * 1024),
    mk(5432, "TCP", "0.0.0.0", 2880, "postgres.exe", 64 * 1024 * 1024),
    mk(6379, "TCP", "127.0.0.1", 7028, "redis-server.exe", 5 * 1024 * 1024),
    mk(53, "UDP", "127.0.0.1", 8420, "dnsproxy.exe", 3 * 1024 * 1024),
    mk(1900, "UDP", "0.0.0.0", 2216, "svchost.exe", 9 * 1024 * 1024),
    mk(5353, "UDP", "0.0.0.0", 1104, "mDNSResponder.exe", 6 * 1024 * 1024),
  ];
}

// 启动
refresh(true);
