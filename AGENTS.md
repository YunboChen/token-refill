# Token Refill · AGENTS.md

本文件记录项目的长期属性与不可随意突破的约束。临时状态（当前分支、待办、某次发布的进度）不写在这里。

## 1. 项目属性

- Tauri 2 + Rust 桌面应用；前端 React 19 + TypeScript + Vite；依赖用 pnpm 12 管理，Node.js ≥ 22.12（CI 用 24）。
- 本地优先：额度查询、状态持久化、系统通知、开机自启、版本检查都在本机完成，没有自建后端、账号体系或云同步。
- 支持平台：macOS（首发，Apple Silicon 与 Intel 分别提供 DMG，最低 12.0）与 Windows x64（MSVC）。其他平台未发布。
- 应用标识 `dev.tokenrefill.desktop`；数据目录由 Tauri `app_data_dir()` 决定：
  - macOS `~/Library/Application Support/dev.tokenrefill.desktop/`
  - Windows `%APPDATA%\dev.tokenrefill.desktop\`
  - 目录内：`state.json`（设置、账户、额度快照、提醒去重、悬浮球当前套餐与位置）、`codex-profiles/<accountId>/`（独立账户的 `CODEX_HOME`）、`api-profiles/<accountId>/`（API Key 账户凭证）、`bailian-profiles/<accountId>/`（百炼控制台令牌）、`relay-profiles/<relayId>/`（手动填写的中转站凭证 `credentials.json`，或扫描导入的来源引用 `source.json` 与额度快照 `snapshot.json`）与 `logs/window.log`（窗口、托盘生命周期日志，单文件上限 256 KiB，轮转保留一份）。
  - `logs/` 只写固定文案、窗口标签与耗时，用途是排查发布包没有控制台（Windows GUI 子系统丢弃 `eprintln!`）时的“点了没反应”。令牌、Cookie、邮箱、密钥、请求内容一律不得写入。
- 分发渠道分正式版（pro）与测试版（test），共用代码、靠 `src-tauri/tauri.test.conf.json` 覆盖身份：
  - 正式版：`productName` `Token Refill`、identifier `dev.tokenrefill.desktop`、版本 `X.Y.Z`，由 `release.yml` 发布并参与应用内更新。
  - 测试版：`productName` `Token Refill Test`、identifier `dev.tokenrefill.desktop.test`、版本 `<X.Y.Z>-test.g<7位提交>`（CI 构建时注入，不写回源文件），数据目录为 `.../dev.tokenrefill.desktop.test/`；不检查更新、不提供应用内更新，只从 `build-test` artifact 下载。
  - 渠道由 Rust `channel.rs` 按 identifier 是否以 `.test` 结尾判定；前端只消费 `get_app_info().channel`。测试配置不得自带 `version`，正式配置不得使用测试 identifier 或 prerelease 版本（CI 两侧都有断言）。
  - 本地 `pnpm tauri:dev:test` 使用测试标识，可与已安装的正式版同时运行；它与 CI 测试包共用测试数据目录和单实例锁，不要同时启动。
  - 两个版本共用上游 Codex OAuth 的固定回调端口 1455/1457，不要同时发起 Codex 授权；百炼回调使用临时端口，无此限制。
- 已实现业务：Codex 订阅额度查询、多账户官方浏览器授权、百炼 / OpenCode Go / Command Code / Antigravity 额度查询、中转站（New API / Sub2API）密钥与账户额度查询、从 cc-switch 与 Claude Code / Codex 当前配置手动扫描并勾选导入中转站候选、菜单栏常驻与快速查看面板、桌面悬浮球（默认关闭，设置中开启）、系统提醒、开机自启、版本检查、应用内下载与安装更新（仅 NSIS 安装版，下载与安装两段确认）。
- 尚未实现：系统级凭证存储、Windows 免安装 exe 的原地更新。
- 菜单栏行为是产品约定：关闭主窗口只隐藏，应用继续常驻并后台刷新；退出只通过菜单栏菜单或面板中的退出入口。菜单栏面板是独立窗口 `tray-panel`（380×560 点、无边框、透明、置顶），与主窗口共用同一份状态与事件；不要把它扩展成第二个带侧栏的管理窗口。面板在配置里是 `create: false`，第一次点开才创建；面板与主窗口隐藏 60 秒后销毁 WebView 以释放内存，重新打开时重建并等待页面加载。登录流程或更新下载进行期间不得销毁主窗口（前端弹窗与进度只存在于该窗口）。销毁最后一个窗口会让系统请求退出，必须在 `RunEvent::ExitRequested` 里拦截（仅拦截“没有窗口且非程序化退出”的情况），否则应用会在空闲销毁后自行结束。
- 窗口创建不得在托盘 / 窗口事件回调里就地执行：这些回调本身在主线程，`run_on_main_thread` 会内联执行，而创建窗口要等 WebView2 初始化（零 WebView 状态下冷启动更慢，内部还会自行泵消息），会卡住托盘菜单并让重入的点击走进创建分支。显示请求一律先转到后台线程再回主线程。
- 桌面悬浮球同样是独立窗口：`floating-ball`（60×60 点透明置顶，内部边界收起时缩为 24×60）与 `float-popover`（340 点宽、按内容自适应高度），都 `create: false`，只在设置开启时创建，关闭开关或球窗口被关闭时一起销毁。球启用期间不参与 60 秒空闲销毁；弹层在启用期间保留以便复用。球只响应鼠标、不抢键盘焦点；菜单弹层需要焦点以接收 Esc 与失焦。macOS 两者都用非激活 `NSPanel`，但必须是两个面板类：球的面板不能成为 key（否则首次点击会被系统用于取 key 而吞掉），弹层保留 key 能力并在 `sendEvent:` 的 mouseDown 里先取 key 再派发（否则详情卡里的按钮要点两次）。Windows 两者加 `WS_EX_TOOLWINDOW`，球再加 `WS_EX_NOACTIVATE`。吸附按完整屏幕边缘计算，工作区只用于上下夹取和自由位置；虚拟桌面最外侧边缘把 60 点窗口移出屏幕，两块显示器之间的内部边界必须把窗口缩到 24 点宽，两种方式都由前端把露出部分重排成「居中平台标识 + 归一化额度环弧」的 D 形（不是简单裁掉一侧）；收起必须分两段——先保持完整外观滑到边缘，贴边后才切换布局并收窄窗口，位移与形变同时进行会让球还没贴边就显示成吸附状态；任何收缩都不得让球跨到相邻显示器。不要把它扩成第二个管理窗口或增加任意前端窗口控制权限。
- 面板“失焦自动收起”必须以“窗口确实获得过焦点且已过宽限期”为前提：Windows 前台锁会让托盘点击唤出的窗口停在未激活状态，此时收到失焦就收起，用户看到的就是“点了没反应 / 一闪而过”。

## 2. 数据与凭证边界

- 任何令牌、Cookie、密钥、邮箱原文都不得进入前端状态、日志、错误消息、测试样例或提交内容。
- Codex 只通过官方 CLI 的 `app-server --stdio` JSON-RPC 交互；不解析、不复制、不迁移授权文件，令牌由 Codex CLI 维护在隔离目录。
- 独立账户必须使用自己的 `CODEX_HOME`；本机账户只沿用本机登录态，不得修改或覆盖它。
- 移除账户只清理本应用目录，不触发源工具注销，不影响其他账户。
- 中转站手动填写的 API Key 与 New API 访问令牌写入 `relay-profiles/<relayId>/credentials.json`（Unix 目录 0700、文件 0600，Windows 沿用 `%APPDATA%` 按用户 ACL），先写临时文件再原子替换；只保存用户主动填写的凭证。从 cc-switch、Claude Code / Codex 配置扫描到的密钥不得复制到应用目录；勾选导入后只能在 `relay-profiles/<relayId>/source.json` 保存来源引用（来源、provider id、地址、provider 类型与用户主动填写的访问令牌），启动时按引用重新读取源配置，取不到就跳过该账户。扫描只列候选、不联网，必须由用户勾选确认后才查询额度。cc-switch 的 `~/.cc-switch/cc-switch.db` 只能只读打开，不修改、不迁移、不复制其中的数据。New API 访问令牌只用于 `/api/user/self` 与 `/api/subscription/self`，不进入前端状态、日志或错误消息；旧版单令牌需要的 `New-Api-User` 用户 ID 与令牌一并保存，只作为请求头发送，不写入日志。
- 状态文件是脱敏数据，不是凭证；不要把它改成日志、调试导出或可分享格式。
- 应用内更新的 minisign 私钥只保存在 GitHub Secrets 与发布者的离线备份中，不得进入仓库、日志、构建产物或应用数据目录；公钥可以公开。

## 3. 架构与安全约束

- 网络请求放在 Rust 侧（目前是版本检查的 ureq 请求）；前端不直连第三方接口，不为了方便放宽 CSP 的 `connect-src`。
- 不新增 filesystem / shell / http 类 Tauri 插件；**禁止**把任意路径、命令行或 URL 交给系统 shell 拼接执行。更新器插件只从 Rust 侧调用，前端不获得它的 IPC 权限，不属于上述插件类别。
- `open_external_url` 只接受 `https://`、长度受限且无控制字符的地址；Windows 用 `ShellExecuteW`，不得改回 `cmd /C start`（URL 中的 `&` 会被当作命令分隔符）。
- capabilities 保持最小：管理窗口与菜单栏面板使用 `core:default`、`core:event:default`、`core:window:allow-start-dragging`；Windows 自绘标题栏另需 `core:window:allow-minimize`、`allow-toggle-maximize`、`allow-maximize`、`allow-unmaximize`、`allow-close`、`allow-is-maximized`，仅服务于 `TitleBar` 的最小化/最大化/关闭与状态同步，不得再扩大。悬浮球与弹层使用独立的 `floating.json`，只有 `core:default` 与 `core:event:default`。新增能力必须说明理由并验证。
- 状态持久化：带 `schemaVersion` 的 JSON；先写临时文件再原子替换；Unix 权限 0600/0700，Windows 依赖 `%APPDATA%` 的按用户 ACL。改动 schema 必须处理旧文件与损坏文件（备份后回退），不能静默覆盖；文件由更高版本写入时只读沿用、提示升级，不得改名备份或回写。
- 不引入遥测、云同步、远程配置；版本检查与更新清单只请求 GitHub Releases 的公开数据，固定 User-Agent，不携带账户或本机信息。
- 应用内更新由 Rust 侧 `tauri-plugin-updater` 完成，前端不授予 `updater:*` 能力，也没有下载/安装的自动入口：下载与安装是两个独立命令，必须由用户点击触发。更新包必须带 minisign 签名，客户端校验通过才允许安装；私钥只存在于 GitHub Secrets，公钥写在 `plugins.updater.pubkey`。

## 4. 平台适配

### Windows

- Rust 目标必须是 `stable-x86_64-pc-windows-msvc`；GNU 目标缺 `dlltool.exe`，不可用。
- Codex 可执行文件查找要覆盖 `codex.exe` / `.cmd` / `.bat` / `.com`；用户目录优先 `USERPROFILE`（`HOME` 可能被 Git Bash 设成 POSIX 路径）。新增安装位置时同时考虑 PATH、版本管理器与包管理器默认目录。
- 所有 Windows 构建（含 debug）使用 GUI 子系统（`#![cfg_attr(windows, windows_subsystem = "windows")]`）；不要让 debug 包重新出现控制台窗口。终端启动时 stdio 仍应可继承，日志与 panic 可见。
- 主窗口关闭系统装饰，由前端 `TitleBar` 自绘标题栏（拖拽、最小化、最大化、关闭）；关闭按钮仍走既有的「隐藏不退出」逻辑。平台差异在 Rust `setup()` 里用 `#[cfg(windows)]` 调 `set_decorations(false)`，不要在 `tauri.windows.conf.json` 覆盖 `app.windows`——平台配置走 JSON Merge Patch，数组会被整体替换。主窗口在 `tauri.conf.json` 中以 `visible: false` 创建：Windows 等前端画出标题栏后 emit `main-ui-ready`、Rust 侧再 `show()`（另有 5 秒兜底），避免 React 首绘前露出约 1 秒的系统标题栏；macOS 在 `setup()` 中直接显示，行为与之前一致。前端通过 `get_app_info().platform` 切换标题栏布局。菜单栏快速面板在 Windows 上按托盘矩形与显示器可用区域定位到任务栏上方，不使用 macOS 的 `NSScreen` 定位分支。
- 打包只产出：免安装单文件 exe（`--no-bundle`）与 NSIS 安装器（`tauri.windows.conf.json` 把 `bundle.targets` 固定为 `["nsis"]`）。**不产出、不发布 MSI**。
- Codex 登录回调端口由上游固定为 1455（1457 仅在占用时回退），本应用不能改；遇到 `os error 10013` 只提示用户释放端口，不要尝试修改 CLI 行为。
- 悬浮球与弹层在 Windows 上通过 `GetWindowLongPtrW` / `SetWindowLongPtrW` 加 `WS_EX_TOOLWINDOW`，球再加 `WS_EX_NOACTIVATE`；弹层需要 `set_focus` 才能收到 Esc 与失焦。透明窗口的渲染、拖动与多显示器吸附必须在 Windows 实机验证。

### macOS

- 发布构建分别使用 `--target aarch64-apple-darwin` 与 `--target x86_64-apple-darwin`，各产出一个 `.dmg`；不合并 Universal fat binary（体积与构建成本都会翻倍）。最低系统 12.0；当前 ad-hoc 签名、未公证，不得把产物描述为已签名/已公证。
- CI 使用 `cargo tauri` 而不是 `pnpm tauri`：Node 版 CLI 下 libuv 会关闭继承的 fd 0，`actool` 的 helper 崩溃，`.icon` 编不出 `Assets.car`（tauri#15315）。改回 `pnpm tauri` 前必须验证打包产物里存在 `Assets.car` 与 `CFBundleIconName`。
- 主窗口使用系统 Overlay 标题栏（`titleBarStyle: "Overlay"` + `hiddenTitle`），红绿灯与拖拽交给系统；侧栏顶部保留 44px 拖拽安全区，不要改成自绘标题栏。
- 不引入需要关闭 Gatekeeper 或降低系统安全设置的常规安装步骤。
- 应用内更新替换 `.app` 后仍是 ad-hoc 签名，cdhash 会变化，通知权限、开机自启可能需要用户重新授权；在实机验证前不得把它描述成无感更新。
- 菜单栏面板用 Cocoa 坐标定位（`NSScreen` / `NSEvent` / `NSWindow`），因为 Tauri 在 Retina 与普通屏混排时的物理坐标不可靠。定位与显示必须经 `run_on_main_thread` 执行；不要改成直接调用 `set_position(PhysicalPosition)`。
- 悬浮球与套餐弹层同样转换为非激活 `NSPanel`：球不调用 `make_key_window`，菜单弹层调用它；空间属性使用 `CanJoinAllSpaces | FullScreenAuxiliary`。吸附位置按完整屏幕计算，不按工作区，否则左侧 Dock 会让球停在 Dock 外侧。
- 菜单栏图标使用单色模板图 `src-tauri/icons/tray-template.png` 并开启 `icon_as_template`；不要再改回彩色应用图标或依赖图标文字标题。

### 开发服务器

- 端口固定 5173（Vite）与 5174（HMR），三处必须同步：`vite.config.ts`、`tauri.conf.json` 的 `build.devUrl` 与 `app.security.devCsp`。
- 改动端口时检查 Windows 的 Hyper-V/WinNAT 预留区间（`netsh interface ipv4 show excludedportrange protocol=tcp`）。

## 5. 版本与发布

- 三处版本必须一致：`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`；改 `Cargo.toml` 后同步 `Cargo.lock`。CI 会校验，不一致直接失败。
- 发布由 `.github/workflows/release.yml` 完成，触发条件：`main` 分支推送且版本号变化、`v*` tag 推送、手动触发。`main` 推送带 `paths` 过滤，只有 `src-tauri/tauri.conf.json` 变更才启动 workflow（提升版本号必然改这个文件），版本未变化时仍跳过全部构建；不要为「省一次 Actions 记录」移除该过滤，否则普通提交会重新产生空跑记录。
- 工作分支一律从 `main` 切出（`feat/*`、`fix/*`、`ci/*`、`docs/*`），PR 到 `main` 合并；**不要从 `dev` 切分支**。`dev` 只承载日常集成：定期把 `main` 合进来保持同步，不作为任何工作分支的基线。
- 发布只走 `main`：发布相关改动同样从 `main` 切分支并 PR 到 `main`，然后**在 `main` 上**提升版本号再推送。不要把 `dev` 整体合并或快进到 `main`——`dev` 里未完成或未验证的功能不得随发版进入 `main`。发版后把 `main` 合回 `dev`，保持版本号一致。
- 两个平台并行构建，产物上传到同一个 Release（tag `v<版本号>`）：
  - macOS：`Token.Refill_<版本>_macos-arm64.dmg` / `Token.Refill_<版本>_macos-x64.dmg`，以及应用内更新用的 `Token.Refill_<版本>_macos-<架构>.app.tar.gz` 与 `.sig`
  - Windows：`Token.Refill_<版本>_windows-x64.exe`（便携）、`Token.Refill_<版本>_windows-x64-setup.exe`（NSIS，同时作为应用内更新包）与 `.exe.sig`
  - 所有产物文件名必须带 `macos-` / `windows-` 前缀，避免两种平台的 `x64` 产物在下载目录里无法区分。
  - 应用内更新清单 `latest.json`：由 publish job 生成，按 `darwin-aarch64` / `darwin-x86_64` / `windows-x86_64` 写入下载地址与签名
- 应用内更新的 minisign 私钥存放在 GitHub Secrets（`TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`），不进仓库、不写日志；`createUpdaterArtifacts` 只在 CI 用 `--config` 打开，本地构建不依赖私钥。私钥丢失只能换公钥并要求用户手动重装，发布前确认已离线备份。
- Release 构建成功后直接发布（`draft: false`），不设草稿确认环节；推送 `main` 前必须自行确认版本号、产物与签名状态，发布即对用户可见。
- 测试包走 `.github/workflows/build-test.yml`（手动触发，用 `ref` 输入选择任意分支与平台），只上传 workflow artifact，不创建 Release、不打 tag、不改版本号；验证分支不必先合入 `main`，也不得用 `release.yml` 的手动触发代替测试构建。GitHub 只为默认分支（`main`）登记手动触发入口，该工作流文件需保留在 `main` 上；目标分支靠 `ref` 指定，因此不必包含该文件。
- 版本检查只做「比较 + 提示」；应用内更新由用户分别确认下载与安装，更新包经 minisign 校验后才替换。不做后台静默安装，也没有自动回滚：更新出问题只能发新版本或让用户手动重装。

## 6. 开发与验证

- 常用命令：
  - `pnpm install --frozen-lockfile`
  - `pnpm build`（tsc 类型检查 + Vite 生产构建）
  - `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
  - `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings`
  - `pnpm test:rust`（`cargo test --locked`）
- Windows 本机必须使用 MSVC 工具链；`tauri dev` 启动的 `cargo run` 不采纳 `RUSTUP_TOOLCHAIN` 环境变量，需用 `rustup default` 或目录级 override。
- 行为改动要配套测试；依赖 Python 的假 `codex app-server` 端到端用例只在 Unix 运行（`#[cfg(unix)]`），Windows 专属分支需要平台内测试。
- 更新链路无法用单元测试覆盖的部分（签名校验、`.app` 替换、NSIS 静默安装）必须在两个平台用真实 Release 验证：安装旧版本 → 检查 → 下载 → 重启安装 → 确认版本、数据目录与权限状态。
- 改动 `.github/workflows/**` 时由 `lint-workflows.yml` 用 actionlint 做静态检查（固定版本 1.7.12 + sha256 校验，只读权限，不构建不发版）；本地改完也应跑一次 `actionlint`，且本机需装 `shellcheck`（否则 CI 的 shellcheck 报错在本地复现不出来），不要靠「下次发版」验证。
- 提交信息使用 Conventional Commits（类型: 中文描述），不添加无关的 `Co-Authored-By`。
- 不提交 `node_modules/`、`dist/`、`src-tauri/target/`、生成物或本机状态文件。
- 改动影响范围、限制或使用方式时，同步更新 README 与 `docs/`（requirements / software-design / development-plan / windows-support）。
- 界面文案、代码注释与文档以中文为主；不要引入未经确认的第三方品牌表述。

## 7. 明确不做

- 不读取浏览器 Cookie、系统钥匙串或其他工具的内部凭证文件来替代官方授权；本机登录态只通过工具官方 CLI 命令读取（Codex app-server、`opencode auth export`、`agy /usage`）。
- 不把「检查到新版本」实现为静默下载或静默安装；后台只做版本检查，下载与安装必须由用户在界面上分别确认。
- Windows 免安装单文件 exe 不做原地替换（没有安装目录可交换），只提供发布页跳转；应用内更新仅面向 NSIS 安装版，通过注册表 `InstallLocation` 与当前 exe 路径判断安装形态。
- 不为尚未确认的平台/套餐预建复杂抽象层。
- 不用真实账户、真实额度或原始响应做测试样例、截图或发布材料。
