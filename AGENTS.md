# AGENTS.md

面向在本仓库中工作的 AI Agent / 开发者的操作说明。项目功能介绍见 [README.md](README.md)。

## 1. 项目概览

KillPorts 是基于 **Tauri 2 + Rust** 的桌面工具（主攻 Windows）：枚举本机全部监听端口
（TCP `LISTEN` + UDP 已绑定），关联进程名 / PID / 可执行路径 / 内存占用，并能强制结束占用端口的进程。

- **前端**：Vite + TypeScript，**无框架**——DOM、表格、弹窗、Toast 全部手写
- **后端**：Rust；端口枚举用 `netstat2`，进程信息与强杀用 `sysinfo`，提权检测用 `windows-sys`
- 前后端仅通过 4 个 Tauri command 通信，没有 HTTP 服务、数据库或持久化状态
- 文档站：`docs/`（GitHub Pages，纯静态，无构建步骤）

## 2. 目录结构

```
index.html                 前端唯一页面（全部 DOM 结构与元素 id）
src/main.ts                前端全部逻辑（单文件，约 300 行）
src/styles.css             前端全部样式（浅色主题，主色 #4f6bf6）
vite.config.ts             固定 127.0.0.1:1420
tsconfig.json              strict: true，include 只有 src
src-tauri/src/lib.rs       Tauri 命令注册（只注册，不写业务逻辑）
src-tauri/src/ports.rs     后端全部业务逻辑 + 单元测试
src-tauri/src/main.rs      入口，仅调用 kill_ports_lib::run()
src-tauri/tauri.conf.json  窗口 / devUrl / 打包配置
src-tauri/capabilities/    Tauri 2 权限能力声明
src-tauri/Cargo.toml       Rust 依赖与 release 优化配置
scripts/make_icon.py       纯标准库生成 app-icon.png（超采样绘制）
docs/                      GitHub Pages 静态项目站
.github/workflows/release.yml  tag 触发自动打包发版
```

生成物与依赖目录，**只读、不要手改**：`node_modules/`、`dist/`、`src-tauri/target/`、`src-tauri/gen/`。

## 3. 常用命令

| 目的 | 命令 | 说明 |
| --- | --- | --- |
| 安装依赖 | `npm install` | |
| 类型检查 + 构建前端 | `npm run build` | 等价于 `tsc --noEmit && vite build` |
| 纯前端预览 | `npm run dev` | 只起 Vite，浏览器中走 mock 数据 |
| 桌面应用开发 | `npm run app:dev` | `tauri dev`，带热更新 |
| 打包发布 | `npm run app:build` | 产出 release exe 与 NSIS 安装包 |
| 后端单元测试 | `cd src-tauri && cargo test` | 枚举真实端口并校验排序 |

Rust 侧单独检查在 `src-tauri` 目录执行 `cargo check` / `cargo build`。

## 4. 关键约定（改动前必读）

### 4.1 devUrl 必须写 127.0.0.1，不能写 localhost
`tauri.conf.json` 的 `devUrl` 固定 `http://127.0.0.1:1420`，`vite.config.ts` 固定
`host: "127.0.0.1"` + `strictPort: true`。Windows WebView2 会把 `localhost` 优先解析到 IPv6 `::1`，
而 Vite 只监听 IPv4 → 开发模式直接白屏。**不要为了"更通用"改成 localhost 或 0.0.0.0。**

### 4.2 前端改一处通常要改三处
没有框架也没有路由：增删界面元素要同时改 `index.html`（结构）、
`src/main.ts`（顶部的 `$("#id")` 取引用 + 逻辑）、`src/styles.css`（样式）。
`$` 是 `document.querySelector` 的窄封装，**查不到会返回 null 并在后续步骤抛错**，
所以元素 id 变更必须三处同步。

### 4.3 PortInfo 字段必须前后端同步
后端 `ports.rs` 的 `PortInfo` 带 `#[serde(rename_all = "camelCase")]`，前端 `src/main.ts`
的 `interface PortInfo` 与之逐字对应（`local_addr` ↔ `localAddr`、`process_name` ↔ `processName` …）。
新增字段时两边一并修改，否则渲染会静默读到 `undefined`。

### 4.4 新增 command 必须注册
新 Tauri command 要在 `lib.rs` 的 `tauri::generate_handler![...]` 中注册，前端用
`invoke("<name>", { args })` 调用（参数名 camelCase）。现有命令：
`list_listening_ports` / `kill_process` / `is_elevated` / `relaunch_as_admin`。
`lib.rs` 保持"只注册、不实现"的分工。

### 4.5 不要破坏浏览器预览路径
`main.ts` 用 `inTauri = "__TAURI_INTERNALS__" in window` 判断运行环境，非 Tauri 环境走 `mockData()`。
`npm run dev` 的纯浏览器体验依赖这个分支，新增 IPC 调用时需同时处理两条路径。

### 4.6 安全不变量（不得删除或放宽）
`ports.rs::kill_process` 内置保护：拒绝结束自身进程、拒绝 PID 0 / 4（内核与 System）；
前端结束前必须经过二次确认弹窗。这是产品对用户的承诺，重构时必须保留。

### 4.7 用户可见文案保持简体中文
UI 文案、错误信息、README、docs 站均为简体中文，且错误信息会直接展示给用户。
新增用户可见文案请用中文。

### 4.8 权限声明
`src-tauri/capabilities/default.json` 目前只声明 `core:default`（未引入 shell / fs 等插件）。
若引入 Tauri 插件，需要同时补 Cargo 依赖、`lib.rs` 的插件注册以及这里的 permissions。

### 4.9 系统进程判定与「隐藏系统进程」开关
后端在 `ports.rs` 为每条记录计算 `system` 字段，满足任一条即视为系统进程：

1. PID 为 0（System Idle）或 4（System / Windows 内核）；
2. 可执行文件位于 `%SystemRoot%` / `%windir%` 之下（大小写与分隔符不敏感，并兼容
   `\\?\` 扩展长度前缀）；
3. 拿不到可执行路径，但进程名命中内核组件清单（`System`、`Registry`、`svchost.exe` 等）。

若给出了明确路径且不在系统目录（如用户目录下同名的 `D:\tools\svchost.exe`），按普通程序处理；
进程信息完全解析不出来时保持 `system = false`——宁可保持可见，也不误伤用户程序。

前端工具栏的「隐藏系统进程」开关**只是展示过滤**（`filtered()` 里的 `hideSystem && p.system`），
不影响扫描结果，也不限制结束进程的能力。

修改判定规则时：保留 `system` 字段名（前端依赖它），并跑 `cargo test`
——其中 `test_is_system_process_classification` 覆盖分类边界，真实端口用例会断言
「系统目录下的进程」与「PID 4 占用的端口」必须被标记为系统进程。

> 注意：受限沙箱（如 DSH 运行环境）下 sysinfo 只能看到当前用户会话的进程，
> `svchost.exe` / `System` 这类系统进程解析不出来，因此该环境里能过滤掉的通常只有 PID 0/4；
> 正常桌面环境会正常显示并过滤 `svchost.exe` 等系统进程。

## 5. 发布流程

版本号需在 **三处保持一致**：`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`。

```bash
git commit -am "chore: release v0.1.1"
git tag v0.1.1
git push origin main --tags
```

`.github/workflows/release.yml`：推送 `v*` tag 或手动 dispatch 触发，在 `windows-latest` 上用
`tauri-action` 构建并创建 Release（当前 `releaseDraft: false`，即直接正式发布）。
注意 `tauri.conf.json` 的 `bundle.targets` 只写了 `nsis`，CI 通过 `--bundles nsis,msi` 额外产出 MSI
——**本地 `npm run app:build` 不会自动得到 MSI**。

发布前本地自检：`npm run build` + `cd src-tauri && cargo test`。

## 6. Agent 工作提示

- **GUI 无法在当前沙箱内验证**：本环境启动桌面应用会被放到隐藏桌面，且沙箱不能创建
  `msedgewebview2` 子进程，界面看起来是"白屏"——这是环境假象，不是代码缺陷。
  涉及 UI 的改动请用 `npm run build`（类型检查）自检，并请用户在本机执行 `npm run app:dev` 确认。
- **沙箱下 `vite build` 可能报 `spawn EPERM`**：esbuild 需要管道 stdio，会被沙箱拦截。
  此时 `tsc --noEmit` 已单独执行通过，属环境限制而非代码问题。
- 改动 Rust 逻辑后优先跑 `cargo test`；改动前端后至少跑 `npm run build`。
- **沙箱构建会把文件打成低完整性标签**：在受限沙箱里执行 `cargo build` 后，产物与源码会带
  `Mandatory Label\Low Mandatory Level`，目录上还会多出 `Everyone:(DENY)(DC)`。这会导致后续启动报
  `Failed to setup app: 拒绝访问。 (os error 5)`（Tauri 创建 WebView2 数据目录失败），甚至让构建脚本
  读不到 `tauri.conf.json`。发现这类报错时先修复权限再排查代码：
  ```powershell
  icacls <项目根> /remove:d "Everyone" /T /C
  icacls <项目根> /setintegritylevel Medium /T /C
  ```

- 仓库没有配置 lint / format 工具，也没有前端测试框架，改动请自行保证 `tsc` 严格模式通过。
- `.workbuddy/` 是本地记忆目录，已在 `.gitignore` 中，不要提交。
