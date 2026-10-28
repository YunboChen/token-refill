# Windows 兼容设计

本文件记录让同一套源码在 Windows 上构建、启动并查询 Codex 额度的兼容层设计，以及已完成与未完成的验证。需求编号见 [需求文档](./requirements.md)（REQ-007）。

## 1. 范围与结论

- 目标：macOS 首发能力在 Windows 10/11 x64 + WebView2 上可用，不引入第二套业务实现。
- 已完成：Codex CLI 查找、app-server 子进程启动、授权链接打开、数据目录与文件写入、窗口标题栏布局。
- 未完成：Windows 安装包签名、Windows 上的真实账户授权、长时间运行验收、最低 Windows 版本结论。
- 构建工具链：必须使用 MSVC 目标。GNU 目标未验证；本机 GNU 工具链缺 `dlltool.exe`，`cargo check` 直接失败。
- `tauri dev` 由 CLI 启动的 `cargo run` 不使用 `RUSTUP_TOOLCHAIN` 环境变量（实测仍选 GNU），需用 `rustup default stable-x86_64-pc-windows-msvc` 或对 `src-tauri` 设置目录级 override。
- 开发端口从 1420/1421 改为 5173/5174：本机 Hyper-V/WinNAT 预留了 1377–1576，落在该区间的监听会返回 `EACCES`。改端口时需同步 `vite.config.ts` 与 `tauri.conf.json` 的 `devUrl`、`devCsp`。

## 2. 平台差异总览

| 能力 | macOS | Windows | 处理方式 |
| --- | --- | --- | --- |
| Codex 可执行文件 | `codex`，看执行位 | `codex.exe` / `.cmd` / `.bat` / `.com` | 候选名与可执行判定按平台分支（`BINARY_NAMES`、`is_executable`） |
| 用户目录 | `HOME` | `USERPROFILE`（Git Bash 等可能把 `HOME` 设为 POSIX 形式） | `home_dir()`：Windows 先取 `USERPROFILE`，Unix 先取 `HOME` |
| 版本管理器 | nvm：`~/.nvm/versions/node/*/bin` | nvm-windows：`NVM_HOME\v*`；fnm：`%APPDATA%\fnm\node-versions\*\installation` | `version_dirs()` 返回版本根目录与相对子路径 |
| 包管理器默认目录 | Homebrew、`~/.local/bin` | `%APPDATA%\npm`、`%LOCALAPPDATA%\pnpm`、Volta、`.bun\bin`、Scoop shims、OpenAI 官方安装目录、WindowsApps | `extra_search_dirs()` |
| 桌面端内置 CLI | `ChatGPT.app/Contents/Resources/codex-cli/bin`（含 `~/Applications`） | `<安装目录>\resources\codex-cli\bin`；Store 版扫描 `WindowsApps\OpenAI.Codex_*` | `desktop_cli_dirs()` / `find_in_msix_desktop_app()`；排在显式安装的 CLI 之后 |
| 打开授权链接 | `open` | `ShellExecuteW` | 不走 `cmd /C start`，URL 不经 shell 解析 |
| 数据目录 | `~/Library/Application Support/<identifier>` | `%APPDATA%\<identifier>` | `app.path().app_data_dir()`，应用代码不拼平台路径 |
| 目录/文件权限 | 0700 / 0600 | 继承 `%APPDATA%` 的按用户 ACL | Unix 显式设权限位，Windows 不设 |
| 窗口标题栏 | Overlay + 隐藏标题（原生红绿灯） | 自绘标题栏（关闭系统装饰） | `titleBarStyle` / `hiddenTitle` 仅 macOS 生效；Windows 在 `setup()` 中 `set_decorations(false)`，由前端 `TitleBar` 绘制控制按钮与拖拽区 |

## 3. Codex CLI 查找顺序

1. `CODEX_BIN`：显式指定；文件不可执行时直接报 `codex-not-found`，不静默回退。
2. `CODEX_CLI_PATH`：Codex 桌面端使用该变量指定自带 CLI；文件不可执行时忽略并继续查找，避免桌面端留下的失效值阻断本应用。
3. `PATH`：按目录顺序探测候选文件名（Windows 优先 `codex.exe`，其次 `.cmd`、`.bat`、`.com`）。
4. 版本管理器：目录名按字符串降序，跳过没有安装 CLI 的版本目录。
5. 包管理器默认目录：从资源管理器启动的 GUI 进程拿不到终端里临时追加的 `PATH`，因此再按默认安装位置找一遍。
6. Codex 桌面端内置 CLI：只装桌面端、没有单独安装 CLI 时由这些位置兜底；排在最后，用户自己安装的 CLI 仍然优先。

Windows 目标目录：

| 来源 | 路径 |
| --- | --- |
| npm 全局 | `%APPDATA%\npm\codex.cmd` |
| pnpm 全局 | `%LOCALAPPDATA%\pnpm\codex.cmd` |
| Volta | `%LOCALAPPDATA%\Volta\bin\codex.exe` |
| Bun | `%USERPROFILE%\.bun\bin\codex.exe` |
| Scoop | `%USERPROFILE%\scoop\shims\codex.cmd` |
| 官方安装包 | `%LOCALAPPDATA%\Programs\OpenAI\Codex\bin\codex.exe` |
| winget / 应用别名 | `%LOCALAPPDATA%\Microsoft\WindowsApps\codex.exe` |
| 桌面端（安装版） | `%LOCALAPPDATA%\Programs\ChatGPT\resources\codex-cli\bin\codex.exe`；同时探测 `Codex`、`OpenAI\Codex` 目录名 |
| 桌面端（Microsoft Store） | `%ProgramFiles%\WindowsApps\OpenAI.Codex_<版本>_x64__<hash>\app\resources\...`；目录受 ACL 保护，枚举失败时跳过 |
| nvm-windows | `%NVM_HOME%\v*\codex.cmd`，以及 `%NVM_SYMLINK%` |
| fnm | `%APPDATA%\fnm\node-versions\v*\installation\codex.cmd` |

桌面端与独立 CLI 共用 `%USERPROFILE%\.codex` 登录态，因此不需要额外登录；查询仍通过官方 `app-server --stdio` 进行，不读取桌面端的内部状态文件。未找到时返回同一个结构化错误，界面提示安装 CLI 或桌面端、或设置 `CODEX_BIN`。

## 4. 子进程启动

- `AppServer` 与 `LoginSession` 都以 `Command::new(<发现的路径>)` 启动 `app-server --stdio`，只通过 stdio 交换行分隔 JSON-RPC。
- `.cmd` / `.bat` 由 Rust 标准库（≥ 1.77.2）自动经 `cmd.exe` 启动，并对参数做批处理转义，因此不需要在业务代码里手写 `cmd /C`。
- 启动时把 CLI 自身目录与 npm / pnpm / Volta / Bun 等常见全局目录补到子进程 `PATH` 前面：包装脚本要能在同一环境里找到 node，否则会以退出码 127 结束。只改子进程环境，不改应用进程。
- 传参固定为 `app-server --stdio`，不接受任何用户输入拼进命令行；托管账户目录通过 `CODEX_HOME` 环境变量传递。
- 进程读取在后台线程进行，30 秒读超时、10 秒初始化超时与取消标志在两端一致。

## 5. 数据与权限

- 状态文件与账户授权目录都位于 Tauri 的 `app_data_dir()`，Windows 下为 `%APPDATA%\dev.tokenrefill.desktop\`，其中 `state.json`、`codex-profiles\<accountId>\`、`bailian-profiles\<accountId>\` 与 `api-profiles\<accountId>\` 与 macOS 结构完全相同。Antigravity 的 `agy` 查找覆盖 `AGY_BIN`、`PATH` 与 `%LOCALAPPDATA%\agy\bin`，与其他 CLI 一样按 `.exe` / `.cmd` / `.bat` / `.com` 判定可执行文件。
- 状态写入仍是「临时文件 → 原子替换」。Windows 上 `fs::rename` 使用 `MoveFileEx(REPLACE_EXISTING)`，可覆盖已存在的状态文件，无需额外分支。
- Unix 继续对状态文件设 0600、对授权目录设 0700。Windows 没有对应权限位，目录继承 `%APPDATA%` 的默认 ACL（当前用户、SYSTEM、Administrators）。若用户把 `APPDATA` 重定向到权限更宽的目录，本应用不会额外收紧 ACL；这一点视为已知限制，而不是已实现的加固。
- 应用不读取、不复制令牌：授权文件始终由 Codex CLI 在该目录内写入。

## 6. 打开授权链接

- 只接受 `https://` 且长度 ≤ 2048、无控制字符与空白的地址。
- macOS 用 `open`，Linux 用 `xdg-open`，Windows 用 `ShellExecuteW("open", url)`。
- 不使用 `cmd /C start "" <url>`：授权 URL 的查询串含 `&`，会被 `cmd.exe` 当命令分隔符截断甚至执行额外命令。`ShellExecuteW` 把 URL 作为独立参数交给系统，无需 shell 转义。

### 6.1 授权回调端口

- Codex CLI 的 OAuth 回调服务固定监听 `127.0.0.1:1455`；1457 只在 1455 返回 `AddrInUse` 时回退（`codex-rs/login/src/server.rs` 的 `DEFAULT_PORT` / `FALLBACK_PORT`）。app-server 的 `account/login/start` 没有端口参数，应用无法改端口。
- Windows 的 Hyper-V/WinNAT 会把若干 TCP 段划进预留区间，落在其中的监听返回 `WSAEACCES`（`os error 10013`），且不会被当作 `AddrInUse`，因此直接失败。本机预留段为 1377–1576，1455/1457 都在其中；同样的命令 `codex login` 也会失败，与本应用无关。
- 修复方式（管理员，全局生效）：停止 WinNAT → 把 1455–1457 加为管理员排除段 → 重新启动 WinNAT。管理员排除段不会再被动态分配，应用可以绑定。

  ```powershell
  net stop winnat
  netsh int ipv4 add excludedportrange protocol=tcp startport=1455 numberofports=3
  net start winnat
  ```

- 应用在 `account/login/start` 错误里检测到 `10013` 时，返回 `login-port-blocked` 及上述命令，而不是直接展示系统错误文本。
- macOS 上没有该预留机制；Linux 通常也不需要处理。

## 7. 窗口与前端

- `tauri.conf.json` 里的 `titleBarStyle: "Overlay"` 与 `hiddenTitle` 只在 macOS 生效（Tauri 在 macOS 分支才应用这两个字段）。Windows 保持原生窗口配置，在 Rust `setup()` 中用 `#[cfg(windows)]` 调 `set_decorations(false)` 关闭系统边框，由前端 `TitleBar` 承担拖拽、最小化、最大化和关闭；不在 `tauri.windows.conf.json` 覆盖 `app.windows`，因为平台配置走 JSON Merge Patch，数组会被整体替换。
- 所有 Windows 构建（含 debug）都使用 GUI 子系统（`#![cfg_attr(windows, windows_subsystem = "windows")]`）。此前 debug 构建是控制台子系统，从资源管理器双击会额外拉起一个控制台窗口（默认由 Windows Terminal 承载）；改为 GUI 后不再出现。从终端运行 `cargo run` / `tauri dev` 时 stdio 仍被继承，日志与 panic 照常输出。
- GUI 子系统进程没有控制台，启动 `codex.exe` 这类控制台程序（以及 `.cmd` 背后的 `cmd.exe`）时 Windows 会为子进程新建控制台窗口。`cli::apply_no_window` 给所有外置 CLI 子进程加 `CREATE_NO_WINDOW`，额度刷新与授权等待期间不再出现黑色控制台窗口；子进程的 stdin/stdout 管道不受影响。
- 单实例由 `tauri-plugin-single-instance` 保证，标识来自应用 `identifier`：免安装 exe 与 NSIS 安装版同时启动也只会保留一个进程，第二次启动把已有窗口从最小化恢复并置顶。
- 前端启动后调用 `get_app_info` 取 `platform`，写到根节点 `data-platform`；首次渲染先用 UA 同步判断，避免 Windows 自绘标题栏首帧缺失。Windows 渲染 `TitleBar`，macOS 使用 44px 红绿灯安全区，页面头仍作为拖拽区。
- 取不到平台信息时保持 macOS 布局；浏览器预览不请求平台信息。

## 8. 打包

- Windows 产物只有两种：免安装单文件 exe（`pnpm tauri build --no-bundle`）与 NSIS 安装器（`pnpm tauri build` 或 `--bundles nsis`）。`src-tauri/tauri.windows.conf.json` 把 `bundle.targets` 收窄为 `["nsis"]`，所以本地构建与 CI 都不再生成 MSI。
- 免安装 exe 约 12 MiB（含版本检查与更新器的 TLS 依赖），前端资源已内嵌，运行依赖系统 WebView2（Win10/11 通常已预装），数据仍写入 `%APPDATA%\dev.tokenrefill.desktop\`。
- 应用内更新只面向 NSIS 安装版：CI 用 `pnpm tauri build --bundles nsis --config <临时配置文件>` 构建（PowerShell 会吞掉内联 JSON 的双引号，因此把 `{"bundle":{"createUpdaterArtifacts":true}}` 写入临时文件），v2 updater 直接把 NSIS 安装器作为更新包，并在旁边产出 `.exe.sig`；免安装 exe 没有安装目录可交换，只保留发布页跳转。客户端通过注册表 `Software\Microsoft\Windows\CurrentVersion\Uninstall\Token Refill` 的 `InstallLocation` 与当前 exe 所在目录比对，判断是否为安装版；两处不一致或读不到时按免安装版处理。
- Windows 构建由 `.github/workflows/release.yml` 的 `build-windows` job（`windows-latest`、MSVC、Node 24）完成，只在版本号变化、tag 推送或手动触发时运行（`main` 推送另有 `paths` 过滤，仅 `src-tauri/tauri.conf.json` 变更才启动 workflow）：先 `pnpm test:rust`，再用 `pnpm tauri build --bundles nsis` 生成便携 exe、NSIS 安装器与 updater 包，最后由 `softprops/action-gh-release@v3` 统一发布。该 job 已在 GitHub Actions 实际运行通过；安装、卸载与升级验收仍未完成，尤其是 NSIS 静默安装（`/P /UPDATE`）与注册表判断需在 Windows 机器上实测。
- 代码签名与发布渠道未定：当前未配置 Authenticode 证书，两种产物都未签名，首次运行会有 SmartScreen 提示，需要在发布说明中写明。更新包的完整性由独立的 minisign 签名保护，不依赖 Authenticode。
- 最低 Windows 版本、架构（x64 / arm64）与实际安装验收仍待确认；当前 CI 只构建 x64。

## 9. 验证记录

2026-10-23，Windows 10.0.26300 x64，MSVC 目标：

| 项目 | 结果 |
| --- | --- |
| `cargo check --all-targets` | 通过 |
| `cargo test` | 30 项通过；合并 `origin/dev` 后全套 47 项通过。仅 Unix 可跑的两个 Python 假 app-server 用例按平台跳过 |
| `cargo clippy --all-targets` | 仅 1 条既有告警（`apply_login_success` 参数 8 个，与本改动无关） |
| `tsc --noEmit` + `vite build` | 通过 |
| 真实 CLI 发现与握手 | 找到 `...\Programs\OpenAI\Codex\bin\codex.exe`；`initialize`、`account/read` 成功返回，未登录时额度请求返回 `auth-expired`，与 macOS 行为一致 |
| 应用启动 | `target\debug\token-refill.exe` 启动后保持运行，生成 `%APPDATA%\dev.tokenrefill.desktop\state.json`，调度器写入查询状态 |
| `pnpm tauri dev` | Vite 监听 5173，Tauri 编译并启动应用窗口（`src-tauri` 使用 MSVC 目录级 override） |
| 授权回调端口 | 修复前绑定 1455 返回 `10013`；把 1455–1457 加为管理员排除段后，真实 `LoginSession::start` 返回 `redirect_uri=127.0.0.1:1455` 的授权地址，取消流程正常 |
| Windows 布局规则 | Chromium 中 `data-platform="windows"` 时安全区高度 44px → 10px，页面头贴顶 |
| `release.yml` | `actionlint` 校验通过；Windows job 尚未在 GitHub Actions 实际运行 |
| Windows 产物构建 | 本机 `--no-bundle` 产出 12.1 MiB 免安装 exe，启动后保持运行；`pnpm tauri build` 只产出 NSIS 安装器（3.20 MiB），`bundle/` 下无 msi 目录 |

尚未验证：

- NSIS 安装器的安装、卸载与升级流程（构建已在本机通过）。
- 已完成授权的账户在 Windows 上查询额度（本机 CLI 未登录，只验证到授权失效路径）。
- 应用窗口的实际渲染与视觉细节（仅确认进程启动与状态写入，未截图窗口）。
- 菜单栏/托盘图标、快速查看面板的显示、定位（任务栏上方）、失焦收起与关闭主窗口后常驻尚未在 Windows 实机验证。
- 自绘标题栏在 Windows 的实机表现：`set_decorations(false)` 是否出现原生边框闪烁、拖拽与边缘缩放是否正常、双击最大化、关闭后常驻、最大化态与不同缩放比例下的布局。
- Windows 10 具体小版本、arm64、长时间运行与休眠恢复。
