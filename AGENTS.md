# Token Refill · AGENTS.md

本文件记录项目的长期属性与不可随意突破的约束。临时状态（当前分支、待办、某次发布的进度）不写在这里。

## 1. 项目属性

- Tauri 2 + Rust 桌面应用；前端 React 19 + TypeScript + Vite；依赖用 pnpm 12 管理，Node.js ≥ 22.12（CI 用 24）。
- 本地优先：额度查询、状态持久化、系统通知、开机自启、版本检查都在本机完成，没有自建后端、账号体系或云同步。
- 支持平台：macOS（首发，Apple Silicon + Intel 通用包，最低 12.0）与 Windows x64（MSVC）。其他平台未发布。
- 应用标识 `dev.tokenrefill.desktop`；数据目录由 Tauri `app_data_dir()` 决定：
  - macOS `~/Library/Application Support/dev.tokenrefill.desktop/`
  - Windows `%APPDATA%\dev.tokenrefill.desktop\`
  - 目录内：`state.json`（设置、账户、额度快照、提醒去重）与 `codex-profiles/<accountId>/`（独立账户的 `CODEX_HOME`）。
- 已实现业务：Codex 订阅额度查询、多账户官方浏览器授权、系统提醒、开机自启、版本检查。
- 尚未实现：百炼 Token Plan / Coding Plan、菜单栏常驻、系统级凭证存储、应用内自动下载安装更新。

## 2. 数据与凭证边界

- 任何令牌、Cookie、密钥、邮箱原文都不得进入前端状态、日志、错误消息、测试样例或提交内容。
- Codex 只通过官方 CLI 的 `app-server --stdio` JSON-RPC 交互；不解析、不复制、不迁移授权文件，令牌由 Codex CLI 维护在隔离目录。
- 独立账户必须使用自己的 `CODEX_HOME`；本机账户只沿用本机登录态，不得修改或覆盖它。
- 移除账户只清理本应用目录，不触发源工具注销，不影响其他账户。
- 状态文件是脱敏数据，不是凭证；不要把它改成日志、调试导出或可分享格式。

## 3. 架构与安全约束

- 网络请求放在 Rust 侧（目前是版本检查的 ureq 请求）；前端不直连第三方接口，不为了方便放宽 CSP 的 `connect-src`。
- 不新增 filesystem / shell / http 类 Tauri 插件；**禁止**把任意路径、命令行或 URL 交给系统 shell 拼接执行。
- `open_external_url` 只接受 `https://`、长度受限且无控制字符的地址；Windows 用 `ShellExecuteW`，不得改回 `cmd /C start`（URL 中的 `&` 会被当作命令分隔符）。
- capabilities 保持最小：`core:default`、`core:event:default`、`core:window:allow-start-dragging`。新增能力必须说明理由并验证。
- 状态持久化：带 `schemaVersion` 的 JSON；先写临时文件再原子替换；Unix 权限 0600/0700，Windows 依赖 `%APPDATA%` 的按用户 ACL。改动 schema 必须处理旧文件与损坏文件（备份后回退），不能静默覆盖。
- 不引入遥测、云同步、远程配置；版本检查只请求 GitHub Releases 的公开数据，固定 User-Agent，不携带账户或本机信息。

## 4. 平台适配

### Windows

- Rust 目标必须是 `stable-x86_64-pc-windows-msvc`；GNU 目标缺 `dlltool.exe`，不可用。
- Codex 可执行文件查找要覆盖 `codex.exe` / `.cmd` / `.bat` / `.com`；用户目录优先 `USERPROFILE`（`HOME` 可能被 Git Bash 设成 POSIX 路径）。新增安装位置时同时考虑 PATH、版本管理器与包管理器默认目录。
- 所有 Windows 构建（含 debug）使用 GUI 子系统（`#![cfg_attr(windows, windows_subsystem = "windows")]`）；不要让 debug 包重新出现控制台窗口。终端启动时 stdio 仍应可继承，日志与 panic 可见。
- 窗口使用系统标题栏；前端通过 `get_app_info().platform` 收窄侧栏顶部留白。
- 打包只产出：免安装单文件 exe（`--no-bundle`）与 NSIS 安装器（`tauri.windows.conf.json` 把 `bundle.targets` 固定为 `["nsis"]`）。**不产出、不发布 MSI**。
- Codex 登录回调端口由上游固定为 1455（1457 仅在占用时回退），本应用不能改；遇到 `os error 10013` 只提示用户释放端口，不要尝试修改 CLI 行为。

### macOS

- 发布构建使用 `--target universal-apple-darwin`，最低系统 12.0；当前 ad-hoc 签名、未公证，不得把产物描述为已签名/已公证。
- CI 使用 `cargo tauri` 而不是 `pnpm tauri`：Node 版 CLI 下 libuv 会关闭继承的 fd 0，`actool` 的 helper 崩溃，`.icon` 编不出 `Assets.car`（tauri#15315）。改回 `pnpm tauri` 前必须验证打包产物里存在 `Assets.car` 与 `CFBundleIconName`。
- 不引入需要关闭 Gatekeeper 或降低系统安全设置的常规安装步骤。

### 开发服务器

- 端口固定 5173（Vite）与 5174（HMR），三处必须同步：`vite.config.ts`、`tauri.conf.json` 的 `build.devUrl` 与 `app.security.devCsp`。
- 改动端口时检查 Windows 的 Hyper-V/WinNAT 预留区间（`netsh interface ipv4 show excludedportrange protocol=tcp`）。

## 5. 版本与发布

- 三处版本必须一致：`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`；改 `Cargo.toml` 后同步 `Cargo.lock`。CI 会校验，不一致直接失败。
- 发布由 `.github/workflows/release.yml` 完成，触发条件：`main` 分支推送且版本号变化、`v*` tag 推送、手动触发。版本未变化时跳过全部构建。
- 两个平台并行构建，产物上传到同一个 draft Release（tag `v<版本号>`）：
  - macOS：通用 `.dmg`
  - Windows：`Token.Refill_<版本>_x64.exe`（便携）+ `Token Refill_<版本>_x64-setup.exe`（NSIS）
- Release 默认是草稿，需要人工确认后发布；未经确认不要改成自动发布。
- 版本检查只做「比较 + 跳转发布页」，不自动下载、不替换正在运行的程序；自动更新需要签名密钥与回滚策略，属另行决策。

## 6. 开发与验证

- 常用命令：
  - `pnpm install --frozen-lockfile`
  - `pnpm build`（tsc 类型检查 + Vite 生产构建）
  - `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
  - `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings`
  - `pnpm test:rust`（`cargo test --locked`）
- Windows 本机必须使用 MSVC 工具链；`tauri dev` 启动的 `cargo run` 不采纳 `RUSTUP_TOOLCHAIN` 环境变量，需用 `rustup default` 或目录级 override。
- 行为改动要配套测试；依赖 Python 的假 `codex app-server` 端到端用例只在 Unix 运行（`#[cfg(unix)]`），Windows 专属分支需要平台内测试。
- 提交信息使用 Conventional Commits（类型: 中文描述），不添加无关的 `Co-Authored-By`。
- 不提交 `node_modules/`、`dist/`、`src-tauri/target/`、生成物或本机状态文件。
- 改动影响范围、限制或使用方式时，同步更新 README 与 `docs/`（requirements / software-design / development-plan / windows-support）。
- 界面文案、代码注释与文档以中文为主；不要引入未经确认的第三方品牌表述。

## 7. 明确不做

- 不读取浏览器 Cookie、系统钥匙串或其他工具的内部凭证文件来替代官方授权；本机登录态只通过工具官方 CLI 命令读取（Codex app-server、`opencode auth export`、`agy /usage`）。
- 不把「检查到新版本」实现为静默下载或静默安装。
- 不为尚未确认的平台/套餐预建复杂抽象层。
- 不用真实账户、真实额度或原始响应做测试样例、截图或发布材料。
