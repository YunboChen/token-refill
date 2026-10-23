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
| 打开授权链接 | `open` | `ShellExecuteW` | 不走 `cmd /C start`，URL 不经 shell 解析 |
| 数据目录 | `~/Library/Application Support/<identifier>` | `%APPDATA%\<identifier>` | `app.path().app_data_dir()`，应用代码不拼平台路径 |
| 目录/文件权限 | 0700 / 0600 | 继承 `%APPDATA%` 的按用户 ACL | Unix 显式设权限位，Windows 不设 |
| 窗口标题栏 | Overlay + 隐藏标题 | 系统标题栏 | `titleBarStyle` / `hiddenTitle` 仅 macOS 生效；前端按平台调整留白 |

## 3. Codex CLI 查找顺序

1. `CODEX_BIN`：显式指定；文件不可执行时直接报 `codex-not-found`，不静默回退。
2. `PATH`：按目录顺序探测候选文件名（Windows 优先 `codex.exe`，其次 `.cmd`、`.bat`、`.com`）。
3. 版本管理器：目录名按字符串降序，跳过没有安装 CLI 的版本目录。
4. 包管理器默认目录：从资源管理器启动的 GUI 进程拿不到终端里临时追加的 `PATH`，因此再按默认安装位置找一遍。

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
| nvm-windows | `%NVM_HOME%\v*\codex.cmd`，以及 `%NVM_SYMLINK%` |
| fnm | `%APPDATA%\fnm\node-versions\v*\installation\codex.cmd` |

未找到时返回同一个结构化错误，界面提示安装 CLI 或设置 `CODEX_BIN`。

## 4. 子进程启动

- `AppServer` 与 `LoginSession` 都以 `Command::new(<发现的路径>)` 启动 `app-server --stdio`，只通过 stdio 交换行分隔 JSON-RPC。
- `.cmd` / `.bat` 由 Rust 标准库（≥ 1.77.2）自动经 `cmd.exe` 启动，并对参数做批处理转义，因此不需要在业务代码里手写 `cmd /C`。
- 传参固定为 `app-server --stdio`，不接受任何用户输入拼进命令行；托管账户目录通过 `CODEX_HOME` 环境变量传递。
- 进程读取在后台线程进行，30 秒读超时、10 秒初始化超时与取消标志在两端一致。

## 5. 数据与权限

- 状态文件与账户授权目录都位于 Tauri 的 `app_data_dir()`，Windows 下为 `%APPDATA%\dev.tokenrefill.desktop\`，其中 `state.json`、`codex-profiles\<accountId>\` 与 macOS 结构完全相同。
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

- `tauri.conf.json` 里的 `titleBarStyle: "Overlay"` 与 `hiddenTitle` 只在 macOS 生效（Tauri 在 macOS 分支才应用这两个字段），Windows 保留系统标题栏与关闭/最小化按钮。
- 所有 Windows 构建（含 debug）都使用 GUI 子系统（`#![cfg_attr(windows, windows_subsystem = "windows")]`）。此前 debug 构建是控制台子系统，从资源管理器双击会额外拉起一个控制台窗口（默认由 Windows Terminal 承载）；改为 GUI 后不再出现。从终端运行 `cargo run` / `tauri dev` 时 stdio 仍被继承，日志与 panic 照常输出。
- 前端启动后调用 `get_app_info` 取 `platform`，写到根节点 `data-platform`。Windows 下侧栏顶部的红绿灯安全区从 44px 收窄到 10px，页面头仍作为拖拽区。
- 取不到平台信息时保持 macOS 布局；浏览器预览不请求平台信息。

## 8. 打包

- Windows 产物只有两种：免安装单文件 exe（`pnpm tauri build --no-bundle`）与 NSIS 安装器（`pnpm tauri build` 或 `--bundles nsis`）。`src-tauri/tauri.windows.conf.json` 把 `bundle.targets` 收窄为 `["nsis"]`，所以本地构建与 CI 都不再生成 MSI。
- 免安装 exe 约 12 MiB（含版本检查的 TLS 依赖），前端资源已内嵌，运行依赖系统 WebView2（Win10/11 通常已预装），数据仍写入 `%APPDATA%\dev.tokenrefill.desktop\`。
- Windows 构建由 `.github/workflows/release.yml` 的 `build-windows` job（`windows-latest`、MSVC、Node 24）完成，只在版本号变化、tag 推送或手动触发时运行：先 `pnpm test:rust`，再 `--no-bundle` 生成便携 exe，随后 `tauri-action` 用 `--bundles nsis` 生成安装器并创建 draft Release，最后用 `softprops/action-gh-release@v3` 把便携 exe 追加到同一个 Release。该 job 目前只通过 actionlint 静态校验，尚未在 GitHub Actions 实际跑过。
- 代码签名与发布渠道未定：当前未配置 Authenticode 证书，两种产物都未签名，首次运行会有 SmartScreen 提示，需要在发布说明中写明。
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
- Windows 10 具体小版本、arm64、长时间运行与休眠恢复。
