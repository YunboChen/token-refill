# Token Refill

用于查看 AI 套餐额度的本地桌面应用。目前支持 Codex 订阅、阿里云百炼 Token Plan 个人版与 Coding Plan 中国站、OpenCode Go、Command Code，以及 Google Antigravity 的额度查询。Codex 与百炼通过官方浏览器授权接入多个独立账户；OpenCode Go 与 Command Code 使用各自的 API Key；Antigravity 沿用本机 Antigravity CLI（`agy`）登录态。

## 技术栈

- Tauri 2 + Rust：桌面壳与本地逻辑。
- React + TypeScript + Vite：前端界面。
- Motion：交互动效，遵循系统的减少动态效果设置。
- CSS Variables + CSS Modules：组件样式，默认跟随系统浅色 / 深色模式。
- pnpm：前端依赖管理。

## 需求与设计文档

首版产品范围已基本收敛，额度查询、多账户授权、菜单栏快速查看与更新链路已进入实现阶段。以下文档区分已确认需求、工程建议与待验证事项。

- [文档索引](./docs/README.md)
- [需求文档](./docs/requirements.md)
- [软件设计文档](./docs/software-design.md)
- [界面设计规范](./docs/design.md)
- [开发计划文档](./docs/development-plan.md)
- [Windows 兼容设计](./docs/windows-support.md)

## 环境准备

- Node.js 22.12+（建议使用当前受支持的 LTS 版本）。
- pnpm 12。
- Rust 稳定版，包含 Cargo、rustfmt 和 Clippy。
- macOS：Xcode Command Line Tools，可使用 `xcode-select --install` 安装。
- Windows：MSVC 构建工具（Visual Studio Build Tools 的「使用 C++ 的桌面开发」）与 WebView2 运行时，参见 [Tauri 环境要求](https://v2.tauri.app/start/prerequisites/)；Rust 目标使用 `x86_64-pc-windows-msvc`。GNU 目标未验证。

macOS 打包配置最低版本为 12.0，Windows 尚未确认最低版本，均不代表已完成对应系统的运行验证。

## 开发

```sh
pnpm install --frozen-lockfile
pnpm tauri dev
```

如果本机已经装着正式版并常驻菜单栏，可以用测试渠道开发，避免两者争用同一个单实例锁与数据目录：

```sh
pnpm tauri:dev:test
```

它使用 `Token Refill Test` 的独立标识与数据目录，可以与正式版同时运行；界面会显示“测试版”标记。不要同时启动两个 dev 脚本——Vite 的 5173/5174 端口是固定的；在两个脚本之间切换会改变 `TAURI_CONFIG`，应用 crate 可能重新编译一次。

工作分支一律从 `main` 切出并 PR 到 `main` 合并；`dev` 只承载日常集成（定期合入 `main` 保持同步），不作为工作分支的基线。

桌面应用启动后进入总览页：先展示 Rust 侧缓存（首次为空时显示查询中），随后按设置自动刷新。总览顶部可在「订阅账户额度」与「中转站账户额度」之间切换；后者在切换时先展示上次成功快照，并把超过刷新间隔的账户自动刷新一次；扫描本地配置改为管理页上的手动入口，两处与「中转站账户管理」页共享同一份结果。订阅额度里的账户类型与账户、以及中转站额度里的账户，都可以用右侧手柄拖动排序（手柄聚焦后也可用上下方向键），顺序保存在本机状态文件，重启和菜单栏面板都沿用同一顺序；菜单栏快速面板里也提供同样的手柄，两处调整会互相同步。Codex 需要本机已安装官方 CLI 或 Codex 桌面端；本机账户沿用已有登录态，未安装或未登录时界面会显示对应错误，其他界面仍可用。

应用同时常驻菜单栏：左键点击菜单栏图标会在图标下方打开快速查看面板，先展示现有额度与更新时间，有中转站账户时再按刷新间隔补查；点击面板外部或按 Esc 收起。macOS 快速面板允许显示在当前桌面或其他应用的全屏空间，不必先回到桌面。右键菜单提供打开主窗口、查看额度与退出。关闭管理窗口只隐藏窗口，应用继续在菜单栏驻留并后台刷新（窗口隐藏 60 秒后会释放 WebView 以降低内存占用，再次打开需要重新加载界面；快速面板首次打开也有一次短暂加载）；macOS 上同时隐藏 Dock 图标与 Cmd+Tab 条目，只保留菜单栏图标，从菜单栏菜单、面板中的打开入口、Finder / Spotlight 或再次启动应用都能恢复管理窗口，恢复时 Dock 图标重新出现。退出必须通过菜单栏菜单或面板右下角的电源按钮，退出后停止调度。

重复启动只保留一个实例：第二次启动会直接退出，并把已运行的窗口恢复、置顶，避免两个进程同时写 `state.json` 和授权目录。单实例锁按应用标识区分，因此正式版与测试版可以各自保留一个实例。

在“账户”页可以添加独立账户：

- **Codex**：应用会为每个账户创建单独的 `CODEX_HOME`，通过 Codex 官方浏览器授权登录。各账户的登录令牌由 Codex CLI 保存在应用数据目录下的独立授权目录中（macOS/Linux 目录权限 0700，Windows 继承 `%APPDATA%` 的按用户 ACL），应用不读取、不复制、也不向前端展示令牌；添加独立账户不会修改本机 Codex 的默认登录态。本机账户也会读取当前本机登录的脱敏邮箱与套餐档位，便于确认正在查看的是哪个账号。
- **百炼**：应用打开阿里云百炼控制台登录页，登录成功后控制台会话令牌通过 `127.0.0.1` 回环地址直接交给 Rust，写入应用数据目录下的 `bailian-profiles/<accountId>/credentials.json`（Unix 目录 0700、文件 0600，Windows 继承 `%APPDATA%` 的按用户 ACL）。令牌不进入前端状态、事件或日志，也不会修改 `bl` CLI 或其他工具的配置。一次授权同时查询 Token Plan 与 Coding Plan。Token Plan 会读取订阅档位（Lite / Essential / Standard / Pro）；账号提示优先使用控制台返回的昵称，昵称为数字时回退为脱敏 UID（控制台令牌接口拿不到阿里云登录名）。Coding Plan 未订阅时接口返回空列表，应用显示为数据缺口；Token Plan 对未订阅账户的返回尚未验证，不据此推断为零额度。
- **OpenCode Go**：启动时先调用本机 `opencode auth export`，若存在活动的 `opencode-go`（或 `opencode`）API Key，会自动接入“本机 OpenCode”账户并在每次刷新时重新读取 CLI 凭据；未检测到时，可在添加账户弹窗粘贴 Go API Key。手动添加的密钥写入 `api-profiles/<accountId>/credentials.json`（Unix 目录 0700、文件 0600）。展示 5 小时、每周与订阅月三个窗口；订阅月按购买日滚动，不推算天数。403 表示该 Key 未开通 Go 订阅。
- **Command Code**：使用 Provider API Key（通常以 `user_` 开头），访问 CLI 同款 `/alpha/billing/*` 与 `/alpha/usage/summary` 接口。展示 5 小时、每周滚动窗口与订阅月额度；月度窗口以“已用 + 剩余”推导上限，订阅接口不可用时仍保留滚动窗口并标注能力缺口。
- **Antigravity**：启动时若本机 `agy`（≥ 1.1.11）已登录并能直接读到额度，会自动接入“本机 Antigravity”账户；未检测到时不创建账户。也可在“添加账户”中手动接入。应用调用官方打印模式 `agy -p /usage --output-format json`，按模型分组展示每周 / 5 小时剩余比例；不读取 agy 的钥匙串、令牌文件或浏览器会话，也不修改其登录状态。找不到 CLI 时可用 `AGY_BIN` 指定路径；低于 1.1.11 时应用会拒绝查询，避免旧版把 `/usage` 当作提示词消耗额度。
- **中转站（New API / Sub2API）**：在“中转站”页点击「添加中转站账户」，填写服务地址与 API Key，应用会依次尝试两种额度接口自动识别平台；也可以点击「扫描本地配置」读取 cc-switch 保存的 provider（Claude Code / Codex 两类）与 Claude Code / Codex 的当前配置，扫描只列出候选、不发起网络请求，勾选确认后才查询额度并加入列表；导入的账户只保存来源引用（不含密钥），启动与每次刷新前都会重新读取源配置；添加后的额度显示在总览的「中转站账户额度」标签页，管理页只负责添加、扫描、编辑账户、补充访问令牌、设置标题与移除。账户卡片上的铅笔按钮可编辑标题、服务地址与 API Key（最多 40 个字符的标题；API Key 留空表示沿用当前密钥）；设置标题后管理页、总览与菜单栏面板都用它作为账户名，留空恢复显示服务地址。标题不是凭证，随凭证一起保存在 `credentials.json` 或 `source.json`。编辑手动账户时会立即重新识别平台并刷新额度；扫描导入的账户保持为源配置引用，服务地址与密钥在启动与每次刷新前从源配置读取，界面只能修改标题；修改源配置后点击刷新即可生效，源配置条目被删除或地址无法再匹配时需要重新导入。New API 优先读取密钥的 `user_quota`（需要 v1.0.0-rc.45 及以上，并在密钥上开启「允许查询用户额度」）；部分站点会直接返回 `quota_scope: "account"` 与已换算的 `display` 块（含 CNY 等币种），应用会直接展示账户余额，不需要额外令牌；也可以填写个人访问令牌（v1.0.0-rc.41 及以上，`profile:read`，读取订阅另需 `wallet:read`），在密钥口径之外读取账户钱包余额与订阅窗口；旧版单令牌站点（例如部分二开站点）还需要同时填写面板个人设置里的用户 ID，作为 `New-Api-User` 请求头发送。对二开站点的响应做了容错：账户口径可来自 `user_quota`、`quota_scope=account` 或 `display`，数字/字符串均可解析，未知字段忽略。Sub2API 使用密钥自身额度；当密钥没有总额度、没有 5h/1d/7d 限速且属于普通分组时，`/v1/usage` 返回账户钱包余额，订阅分组返回账户订阅额度。手动填写的地址、密钥与访问令牌保存在 `relay-profiles/<relayId>/credentials.json`（Unix 目录 0700、文件 0600）；扫描导入的账户不保存密钥，只在同目录的 `source.json` 保存来源引用，启动与刷新时重新读取 cc-switch 数据库或当前配置文件；源配置不存在或已变化导致无法匹配时，刷新会提示原因并保留上次快照，需要重新扫描导入。

网络代理在“设置 → 网络代理”统一配置，三种模式：**跟随系统**（Windows 读取 Internet Settings，macOS 读取 `scutil --proxy`；进程环境变量优先，按协议先取 `HTTP(S)_PROXY`，两者都缺失时才回落到 `ALL_PROXY`，`socks5://` 形式也可用）、**不走代理**（直连）、**指定代理**（手动填写 `http://host:port`）。该设置作用于应用的全部 HTTPS 额度查询、版本检查，以及 `agy`、Codex app-server 等外置 CLI；指定代理不支持 PAC，且不能包含用户名或密码。

外置 CLI 启动时会拿到补齐的 `PATH`：CLI 自身所在目录优先，其次是 nvm / fnm / Volta / Bun / pnpm 与 Homebrew 等常见目录，最后才是进程原有的 `PATH`。从 Finder 或资源管理器启动的 GUI 进程只有系统目录，npm 安装的 `codex`、`opencode` 是 `#!/usr/bin/env node` 脚本，缺了这一层补充会以退出码 127 结束。

百炼额度通过百炼控制台网关读取，依赖控制台会话，不能使用推理 API Key 替代。会话过期后账户页会提示重新授权。

只预览网页：

```sh
pnpm dev
```

访问 `http://127.0.0.1:5173`。浏览器预览使用内置默认状态，不会调用 Rust 或读取真实额度；设置仅在当前页面内存中生效。

### Windows 说明

- Codex CLI 查找顺序：`CODEX_BIN` → `CODEX_CLI_PATH` → `PATH` → nvm-windows / fnm 的版本目录 → npm、pnpm、Volta、Bun、Scoop、`%LOCALAPPDATA%\Programs\OpenAI\Codex\bin` 等默认安装位置 → Codex 桌面端内置 CLI（`%LOCALAPPDATA%\Programs\ChatGPT\resources\codex-cli\bin`、Microsoft Store 版 `WindowsApps\OpenAI.Codex_<版本>\` 等）。候选文件为 `codex.exe`、`codex.cmd`、`codex.bat`、`codex.com`，优先原生可执行文件；只装桌面端、没有单独安装 CLI 时也能查询本机额度（两者共用 `~/.codex` 登录态）。找不到时设置 `CODEX_BIN` 指向完整路径。
- 数据目录为 `%APPDATA%\dev.tokenrefill.desktop\`（测试版为 `%APPDATA%\dev.tokenrefill.desktop.test\`），其中 `state.json` 存设置与快照，`codex-profiles\<accountId>\` 是各独立 Codex 账户的 `CODEX_HOME`，`bailian-profiles\<accountId>\` 与 `api-profiles\<accountId>\` 分别保存百炼控制台令牌和 OpenCode / Command Code 的 API Key，`relay-profiles\<relayId>\` 保存手动填写的中转站凭证，`logs\window.log` 记录窗口与托盘的启停、显示与失败（只在排查问题时需要，不含任何凭证）。
- Antigravity 需要本机可运行的 `agy`，查找顺序为 `AGY_BIN` → `PATH` → `%LOCALAPPDATA%\agy\bin` 等常见目录；额度通过 `agy -p /usage --output-format json` 读取。
- 主窗口使用自绘标题栏：Rust 侧关闭系统装饰，前端 `TitleBar` 负责拖拽、最小化、最大化与关闭；macOS 仍使用系统 Overlay 标题栏，前端按 `get_app_info().platform` 切换布局。Windows 侧标题栏仍需在实机验收。
- 授权链接通过 `ShellExecuteW` 交给系统浏览器，不经过 `cmd.exe`，URL 中的 `&` 不会被当作命令分隔符。
- `src-tauri/tauri.windows.conf.json` 把 Windows 的 `bundle.targets` 收窄为 `["nsis"]`：`pnpm tauri build` 只产出 NSIS 安装器（`-setup.exe`），不再生成 MSI；免安装单文件 exe 用 `pnpm tauri build --no-bundle`。两者尚未做安装与卸载验收。详见 [Windows 兼容设计](./docs/windows-support.md)。

#### Windows 开发排错

- **`EACCES: permission denied 127.0.0.1:5173`**：Hyper-V / WinNAT 会在开机时预留若干 TCP 端口段，落在预留区间的端口无法监听。用 `netsh interface ipv4 show excludedportrange protocol=tcp` 查看本机区间。开发端口在 `vite.config.ts`（`server.port` / `server.hmr.port`）与 `src-tauri/tauri.conf.json`（`build.devUrl`、`app.security.devCsp`）两处，换成空闲端口时需要同时修改。
- **`error calling dlltool 'dlltool.exe': program not found`**：默认 Rust 工具链是 GNU 目标，缺少 MinGW 组件，`tauri dev` 生成的 `cargo run` 也不会采用 `RUSTUP_TOOLCHAIN` 环境变量（本机实测仍走 GNU）。请改用 MSVC：`rustup default stable-x86_64-pc-windows-msvc`，或只对本仓库生效 `rustup override set stable-x86_64-pc-windows-msvc --path src-tauri`（撤销：`rustup override unset --path src-tauri`）。
- **添加账户报 `failed to start login server … (os error 10013)`**：Codex CLI 的 OAuth 回调固定监听 `127.0.0.1:1455`（1457 仅在端口被占用时回退），Windows 预留区间覆盖这两个端口时绑定被拒。以管理员身份执行：

  ```powershell
  net stop winnat
  netsh int ipv4 add excludedportrange protocol=tcp startport=1455 numberofports=3
  net start winnat
  ```

  之后 `netsh interface ipv4 show excludedportrange protocol=tcp` 应出现带 `*` 的 `1455 1457`。这是系统级问题：直接用 `codex login` 也会同样失败，应用会把该错误翻译成这段修复提示；macOS 没有这种预留机制。

## 检查与构建

```sh
# TypeScript 检查和前端生产构建
pnpm build

# Rust 格式、编译、静态检查与测试
cargo fmt --manifest-path src-tauri/Cargo.toml --check
pnpm check:rust
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings
pnpm test:rust

# 本机桌面应用打包
pnpm tauri build
```

首次编译需要下载并构建 Tauri 的 Rust 依赖，耗时通常明显长于后续增量编译。

本地 `pnpm tauri build` 不生成应用内更新包、也不需要更新私钥：`createUpdaterArtifacts` 只在 CI 发布时通过 `--config` 打开。需要本地验证更新包时，自行提供 `TAURI_SIGNING_PRIVATE_KEY` 并加上同样的 `--config`。

`pnpm-lock.yaml` 和 `src-tauri/Cargo.lock` 应随项目提交。正式分发前需要配置自己的应用标识、代码签名与 macOS 公证；当前 `dev.tokenrefill.desktop` 仅作开发标识。

### 应用图标

平图图标源图是 `src-tauri/icons/source.png`（1024 × 1024，带透明底板外轮廓）。替换源图后运行 `pnpm tauri icon src-tauri/icons/source.png` 重新生成整套图标；页头标志是 `src/assets/logo.png`，浏览器标签图标位于 `public/`。

菜单栏模板图标是 `src-tauri/icons/tray-template.png`；测试版使用 `src-tauri/icons/tray-test-template.png`（同一字形缩小后加右下圆点），两者都通过 `include_bytes!` 随包编译，不加入 `bundle.icon`。

macOS 26+ 的自适应分层图标（浅色 / 深色 / 有色 / 降低透明度跟随系统）源文件在 `src-tauri/icons/macos/TokenRefill.icon`，由 `src-tauri/tauri.macos.conf.json` 追加进 `bundle.icon`，只影响 macOS 打包：

- 打包器把 `.icon` 交给 `actool` 编成 `Contents/Resources/Assets.car`，再用 `assetutil` 取回内部图标名写入 `CFBundleIconName`；`CFBundleIconFile`（ICNS）保留，供旧系统回退。
- 缺 `actool`（只装了 Command Line Tools）或 actool 主版本低于 26 时，只记一条日志并跳过 Assets.car，产物仍是平图，构建不失败。
- 这个 `.icon` 是手工生成的草稿：先用 Icon Composer 打开、逐外观检查并重新保存，再 `pnpm tauri build --bundles app`。验收看三处——`.app/Contents/Resources/Assets.car` 存在、`Info.plist` 里有 `CFBundleIconName`、`assetutil --info Assets.car` 里能查到该名字的 `Icon Image` rendition。
- `pnpm tauri dev` 的 Dock 图标是编译期内嵌 ICNS 的单张图，不体现自适应效果；必须用打包后的 `.app` 在 Finder 与 Dock 上验收。
- 菜单栏模板图标 `src-tauri/icons/tray-template.png` 取自 `src/assets/logo.png` 的 alpha 轮廓（单色、透明底），由 Rust 编译期内嵌；替换页头标志后需要同步重新生成，保持图形与留白一致。模板图标只保留 alpha，菜单栏会按浅色/深色自动反色。

## 目录

```text
src/
  App.tsx             管理窗口外壳与分区导航
  App.module.css      界面组件样式
  styles.css          系统字体、主题 token、全局样式
  lib/desktop.ts      带类型的 Rust 状态与命令入口
  lib/store.tsx       统一状态订阅、刷新与偏好保存
  lib/relay.tsx       中转站账户的共享状态与操作
  lib/relayFormat.ts  中转站文案、金额与窗口格式化
  lib/prefs.ts        主题与默认偏好
  panels/             总览、账户、中转站账户管理、设置与菜单栏快速查看
  panels/TrayPanel.tsx 菜单栏快速查看面板（380×560）
  assets/logo.png     页头品牌标志
  main.tsx            React 与 Motion 入口（按窗口 URL 选择主界面或面板）
src-tauri/
  src/lib.rs          Tauri 启动入口与 IPC 命令
  src/state.rs        状态模型、JSON 持久化与刷新调度
  src/codex.rs        Codex app-server 只读适配器
  src/bailian.rs      百炼控制台只读适配器与浏览器登录回调
  src/main.rs         桌面可执行入口
  src/tray.rs         菜单栏图标、快速查看面板定位与常驻行为
  capabilities/       本地窗口与事件权限配置
  icons/              应用图标、1024 源图与菜单栏模板图标
  icons/macos/        macOS 分层图标草稿 TokenRefill.icon
  tauri.conf.json     窗口、构建和 CSP 配置
  tauri.macos.conf.json  macOS 专属打包配置（追加分层图标）
public/               浏览器标签图标、Web manifest
```

## 发布

打包由 [.github/workflows/release.yml](./.github/workflows/release.yml) 在 GitHub Actions 上完成：macOS 分别提供 Apple Silicon（arm64）与 Intel（x64）的 `.dmg`，以及 Windows x64 的免安装单文件 exe 与 NSIS 安装器（`-setup.exe`）。Windows 不产出 MSI。

产物文件名带平台前缀，便于区分：`Token.Refill_<版本>_macos-arm64.dmg`、`Token.Refill_<版本>_macos-x64.dmg`、`Token.Refill_<版本>_windows-x64.exe`（免安装）与 `Token.Refill_<版本>_windows-x64-setup.exe`（NSIS）。

应用内更新另需各平台的更新包与签名：macOS 的 `.app.tar.gz` 及 `.sig`，Windows 的 NSIS 安装器（同时作为更新包）及 `.exe.sig`，再加一份 `latest.json`。清单由 `publish` job 生成并和其它产物一起挂到 Release；应用从 `releases/latest/download/latest.json` 读取版本、下载地址与签名。

发版步骤：

1. 发布相关改动从 `main` 切出分支，PR 到 `main` 并合并。**不要把 `dev` 整体合并到 `main`**：`dev` 可能包含未完成或未验证的功能，发版不应把它们带出去。

2. 在 `main` 上把三处版本号改成同一个值，并重新生成 `Cargo.lock`：

   ```sh
   # package.json / src-tauri/tauri.conf.json 手动改
   cargo check --manifest-path src-tauri/Cargo.toml   # 同步 Cargo.lock
   ```

3. 提交并推送到 `main`：

   ```sh
   git push origin main
   ```

4. 发布完成后把 `main` 合回 `dev`，让两边版本号保持一致。

`main` 分支推送带 `paths` 过滤：只有 `src-tauri/tauri.conf.json` 被改动的推送才会启动 `release` workflow（提升版本号必然改这个文件），普通代码提交不再产生空的 Actions 记录；tag 推送不受该过滤影响。启动后 `prepare` job 会对比这次推送前后的 `src-tauri/tauri.conf.json` 版本号：**只有版本号变化才继续构建**，版本未变时其余 job 全部跳过，不会重复生成或上传产物。也可以显式发版：推送 `v*` tag，或在 Actions 页面**从 `main`** 手动触发 `release`（手动触发不比较版本，总是构建；在其他分支上触发会直接报错，测试构建请用 `build-test`）。`prepare` 还会拒绝低于已发布版本的版本号——`releases/latest` 按发布时间选取，重发旧版本会让用户被更新回旧版。

合并前的检查由 [.github/workflows/ci.yml](./.github/workflows/ci.yml) 承担：PR 与推送到 `main` / `dev` 时，在 macOS 上跑 `pnpm build`、`cargo fmt --check`、`clippy -D warnings` 与 `cargo test`，并在 Windows 上跑 `pnpm test:rust` 覆盖 Windows 专属分支。`main` 的分支保护要求 `ci / check` 与 `ci / check (windows)` 通过（不要求 review）；版本号三处一致仍只在 release 的 `check` 里校验。依赖与 action 版本由 `.github/dependabot.yml` 每周检查。

改 workflow 文件时由独立的 `lint-workflows` workflow 用 [actionlint](https://github.com/rhysd/actionlint) 做静态检查（只监听 `.github/workflows/**` 的推送与 PR，不构建、不发版）：表达式、`needs` 引用、action 输入这类只在运行时才暴露的问题能在提交阶段被发现。本地也可以直接跑 `actionlint`。

构建流程：先在 macOS runner 上跑检查（`pnpm build`、`cargo fmt --check`、`clippy -D warnings`、`cargo test`），随后并行构建 macOS arm64 / x64 与 Windows x64 产物：macOS 用 `cargo tauri` + ad-hoc 签名，并额外生成 `.app.tar.gz` 更新包；Windows 用 MSVC 工具链，先跑 `pnpm test:rust`，再执行一次 `pnpm tauri build --bundles nsis --config <临时配置文件>`（Windows 的 PowerShell 会吞掉内联 JSON 的双引号，CI 把 `{"bundle":{"createUpdaterArtifacts":true}}` 写到临时文件后传入），同时得到免安装 exe、NSIS 安装器与安装器的 `.exe.sig` 更新签名。所有产物都挂到同一个 **Release**（tag 为 `v<版本号>`）并直接发布，没有草稿确认环节；推送到 `main` 前请自行确认版本号与产物。`tauri.conf.json` / `package.json` / `Cargo.toml` 三处版本不一致时构建直接失败；tag 推送还会校验 tag 与版本号一致。

### 测试版与正式版共存

测试包由 `build-test` workflow 手动触发，产物只作为 Actions artifact 提供，不创建 Release、也不参与应用内更新。它在构建时叠加 `src-tauri/tauri.test.conf.json`：

- `productName` → `Token Refill Test`：macOS 安装为 `Token Refill Test.app`；Windows NSIS 安装到 `%LOCALAPPDATA%\Token Refill Test`，并使用独立的卸载键。
- `identifier` → `dev.tokenrefill.desktop.test`：数据目录、单实例锁、WebView2 用户数据、通知权限与开机自启都与正式版隔离。
- 版本号 → `<正式版本>-test.g<7位提交>`（如 `0.2.6-test.g0dd074f`）：由 `meta` job 注入，不写回源文件；`CFBundleShortVersionString` 带测试后缀，`CFBundleVersion` 保持纯数字的基础版本。

测试版不检查更新、不提供下载或安装更新；新测试包需要重新触发 `build-test` 并从 Actions artifact 下载。`meta` job 会校验测试配置的 identifier / productName，防止 `--config` 失效时把正式版身份带进测试包；`release.yml` 也会拒绝测试 identifier 与带 prerelease 的正式版本号。

首次运行测试版是独立数据目录：本机 Codex 账户会自动识别，但独立 Codex 账户、API Key 账户与中转站凭证需要在测试版内重新添加。两个版本不要同时发起 Codex 浏览器授权（上游固定回调端口 1455/1457）；Dock 与 Applications 中两个应用图标相同，菜单栏图标靠测试版右下角的圆点区分。

### 更新签名密钥

应用内更新用 minisign 校验更新包，和 Apple / Windows 代码签名互相独立。首次启用或更换发布仓库时：

```sh
cargo tauri signer generate -w ~/.tauri/token-refill.key
```

把输出的公钥填入 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`，把私钥文件内容与密码分别存为仓库 Secrets `TAURI_SIGNING_PRIVATE_KEY`、`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。私钥丢失后无法再给已发布版本推送更新，只能换公钥并让用户手动重装一次，因此务必离线备份。更新清单本身不签名，但每个更新包都由私钥签名，客户端会先校验再安装。

### 测试版构建

需要在不发版的情况下验证某个分支时，用 [.github/workflows/build-test.yml](./.github/workflows/build-test.yml)：在 Actions 页面手动触发 `build-test`，用 `ref` 输入指定要构建的分支、tag 或提交（默认 `dev`），并可只构建 macOS 或 Windows。

GitHub 只为**默认分支**（本仓库是 `main`）上的工作流文件登记手动触发入口，所以这个文件需要存在于 `main`；要构建的分支由 `ref` 指定，业务代码不必合入 `main`，目标分支本身也不需要包含该文件。

- 只产出 workflow artifact（在本次运行的 Artifacts 区下载），**不创建 Release、不打 tag**，不会覆盖已发布版本；产物保留 14 天。
- 检查与打包步骤和正式发布一致，版本号保持当前值、不在构建时改写。
- 产物名带短提交 SHA（如 `Token.Refill_0.2.1-f4296bf_arm64.dmg`），多个测试包下载到同一目录不会互相覆盖；应用内显示的仍是不带 SHA 的版本号，需要靠文件名区分是哪一个提交。
- 签名状态与正式包相同：macOS ad-hoc 签名未公证、Windows 未签名，首次打开的限制见下一节。

不要用 `release` workflow 的手动触发打测试包——那条路径会直接发布 Release。

### 签名状态

当前未接入 Apple Developer 证书，产物是 ad-hoc 签名、未公证，其他 Mac 首次打开会提示"无法验证开发者"。可以右键 →「打开」，或先移除隔离属性：

```sh
xattr -dr com.apple.quarantine "/Applications/Token Refill.app"
```

接入证书后，在仓库 Secrets 配置 `APPLE_CERTIFICATE`、`APPLE_CERTIFICATE_PASSWORD`、`APPLE_SIGNING_IDENTITY`、`APPLE_ID`、`APPLE_PASSWORD`、`APPLE_TEAM_ID`，tauri-action 会自动签名并公证；同时删除 workflow 里的 `APPLE_SIGNING_IDENTITY: "-"`。

macOS 26 的自适应图标依赖 runner 上的 Xcode 26 `actool`；runner 版本更低时会按上文跳过 `Assets.car`，产物退回平图图标，构建不会失败。

CI 打包刻意走 `cargo tauri` 而不是 `pnpm tauri`：Node 版 CLI 下 libuv 会把继承来的 fd 0 标记为 close-on-exec，`actool` 启动时 stdin 已关闭，其 helper `ibtoold` 崩溃，`.icon` 就编不出 `Assets.car`（[tauri#15315](https://github.com/tauri-apps/tauri/issues/15315)）。`cargo-tauri` 不经过 libuv，所以正常。上游修复 PR 合并并升级 CLI 后，可以改回 `pnpm tauri` 并删掉 workflow 里的安装步骤。

Windows 的免安装 exe 与 NSIS 安装器由 `build-windows` job 用 MSVC 工具链构建。两者都未配置 Authenticode 证书，首次运行会出现 SmartScreen 提示；免安装 exe 依赖系统 WebView2（Win10/11 通常已预装）。接入代码签名证书后可在该 job 补充签名步骤。Windows 包的安装与卸载验收仍未完成。

## 当前范围

已包含基础窗口、系统浅深色、减少动态效果支持、Motion 反馈动画，以及两条真实业务链路：**Codex 订阅额度查询**与**百炼 Token Plan / Coding Plan 额度查询**。

桌面应用启动后先读取 Rust 侧的缓存快照，再按设置自动刷新：默认每 5 分钟一次，可在设置中调整；支持手动刷新，同一账户的在途请求会去重。查询在 Rust 后台线程执行，窗口切换不会重复发起；失败时保留上一次成功数据并标明错误，不把失败显示成零额度。账户、套餐、额度窗口、更新时间与查询状态由统一状态模型管理，偏好保存到应用数据目录的版本化 JSON。

当前 Codex 查询依赖本机已安装的 Codex CLI，或 Codex 桌面端自带的 CLI（桌面端与独立 CLI 共用 `~/.codex` 登录态，只装桌面端即可读取本机额度）。启动时若本机找不到任何 Codex 客户端，总览与账户页不会显示「本机 Codex」账户，也不会反复发起查询；安装客户端后重启应用即可恢复显示。本机账户沿用本机已有登录态；独立账户通过官方授权登录，各自使用隔离的 `CODEX_HOME`，支持添加、改名、重新授权与移除。账户页不再只读。

百炼账户通过控制台浏览器授权接入，不依赖 `bl` CLI；同一账户下的 Token Plan 与 Coding Plan 作为两个独立产品展示。账户页会把每个已开通套餐单独列出（名称、档位、最近更新时间），未开通的套餐不占位。Token Plan 展示订阅月的已用/剩余比例，Coding Plan 保留原始调用次数与 5 小时、周、订阅月三个窗口，不把不同单位合并成一个总数。

开机自启已接入系统登录项：默认关闭，开关只反映系统实际状态；写入失败会如实报错且不改本地状态，启动时以系统登录项为准回写偏好。

低额度、授权失效与重置提醒已接入系统通知：低额度在剩余比例低于设置阈值时发送，同一重置周期只提醒一次；授权失效只在状态从正常跃迁到失效时提醒一次，重复查询与应用重启都不会重复；重置提醒默认关闭，只有查询结果同时确认旧周期结束和新周期开始时才发送，不根据本地倒计时推断。去重记录随状态持久化，设置页提供测试通知入口。macOS 侧改用 `UNUserNotificationCenter`：启动时申请一次系统通知授权，应用位于前台时也会弹出横幅；权限被拒绝或尚未允许时，设置页会给出可直接处理的提示。通知是否展示仍受系统通知设置控制。

设置页的“版本”区块会读取 GitHub Releases 的最新正式版本并与当前版本比较：有新版本时显示版本号；安装版可以点“下载更新”查看进度，下载完成后再点“重启并安装”，两步都由用户显式触发。macOS 用 `.app.tar.gz` 原地替换并重启；Windows 只支持 NSIS 安装版，免安装单文件 exe 会提示前往发布页手动替换。更新包通过 minisign 签名校验后才会安装，安装期间应用会退出并重启。首个带更新器的版本（0.2.3 起）仍需从发布页手动安装一次，之后才能应用内更新。应用启动时也会静默检查一次（只检查，不下载），发现新版本时在侧栏底部版本号旁提示；运行期间每 6 小时自动复查，窗口重新获得焦点时（距上次检查超过 1 小时）也会复查，检查失败后 30 分钟静默重试，避免长时间驻留时错过新版本。百炼账户不做版本或订阅档位的额外推断：Token Plan 只展示平台返回的月度比例与重置时间，Coding Plan 额外保留调用次数与三个窗口。请求只包含固定 User-Agent，不携带账户或本机数据。设置页重复进入复用已有结果，手动点击“检查更新”会重新查询；离线、无正式版本或接口限流时会显示对应提示。macOS 产物仍是 ad-hoc 签名，替换后系统可能重新询问通知权限或开机自启，这是当前签名方式的限制。

尚未实现：

- 独立账户凭证的系统级安全存储（当前使用应用数据目录的私有文件：Codex 由 CLI 维护授权文件，百炼保存控制台会话令牌）。
- 平台限流退避参数、休眠唤醒后的补偿策略。
- Windows 免安装 exe 的应用内更新（NSIS 安装版已支持）。
- 原生 Liquid Glass 与旧系统材质回退。目前没有将 CSS 毛玻璃伪称为系统 Liquid Glass。

前端不应持有账户密钥或在日志中输出凭证。后续平台请求和凭证访问优先放在 Rust / 系统层。

## 许可证

本项目采用 [MIT License](./LICENSE)。
