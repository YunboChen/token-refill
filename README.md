# Token Refill

用于查看 AI 套餐额度的本地桌面应用。目前已完成 Codex 订阅额度查询，并支持通过官方浏览器授权接入多个独立 Codex 账户。

## 技术栈

- Tauri 2 + Rust：桌面壳与本地逻辑。
- React + TypeScript + Vite：前端界面。
- Motion：交互动效，遵循系统的减少动态效果设置。
- CSS Variables + CSS Modules：组件样式，默认跟随系统浅色 / 深色模式。
- pnpm：前端依赖管理。

## 需求与设计文档

首版产品范围已基本收敛，当前进行接入可行性验证与软件设计，尚未开始业务功能实现。以下文档区分已确认需求、工程建议与待验证事项。

- [文档索引](./docs/README.md)
- [需求文档](./docs/requirements.md)
- [软件设计文档](./docs/software-design.md)
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

桌面应用启动后进入总览页：先展示 Rust 侧缓存（首次为空时显示查询中），随后按设置自动刷新。Codex 需要本机已安装官方 CLI；本机账户沿用 CLI 已有登录态，未安装或未登录时界面会显示对应错误，其他界面仍可用。

在“账户”页可以添加独立账户：应用会为每个账户创建单独的 `CODEX_HOME`，通过 Codex 官方浏览器授权登录。各账户的登录令牌由 Codex CLI 保存在应用数据目录下的独立授权目录中（macOS/Linux 目录权限 0700，Windows 继承 `%APPDATA%` 的按用户 ACL），应用不读取、不复制、也不向前端展示令牌；添加独立账户不会修改本机 Codex 的默认登录态。本机账户也会读取当前 CLI 登录的脱敏邮箱与套餐档位，便于确认正在查看的是哪个账号。

只预览网页：

```sh
pnpm dev
```

访问 `http://127.0.0.1:5173`。浏览器预览使用内置默认状态，不会调用 Rust 或读取真实额度；设置仅在当前页面内存中生效。

### Windows 说明

- Codex CLI 查找顺序：`CODEX_BIN` → `PATH` → nvm-windows / fnm 的版本目录 → npm、pnpm、Volta、Bun、Scoop、`%LOCALAPPDATA%\Programs\OpenAI\Codex\bin` 等默认安装位置。候选文件为 `codex.exe`、`codex.cmd`、`codex.bat`、`codex.com`，优先原生可执行文件；找不到时设置 `CODEX_BIN` 指向完整路径。
- 数据目录为 `%APPDATA%\dev.tokenrefill.desktop\`，其中 `state.json` 存设置与快照，`codex-profiles\<accountId>\` 是各独立账户的 `CODEX_HOME`。
- 窗口使用系统标题栏，前端按 `get_app_info().platform` 收窄侧栏顶部留白。
- 授权链接通过 `ShellExecuteW` 交给系统浏览器，不经过 `cmd.exe`，URL 中的 `&` 不会被当作命令分隔符。
- `pnpm tauri build` 在 Windows 上默认产出 MSI 与 NSIS；本项目只发布 NSIS 安装器（`-setup.exe`）与免安装单文件 exe（`--no-bundle`）。两者尚未做安装与卸载验收。详见 [Windows 兼容设计](./docs/windows-support.md)。

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

`pnpm-lock.yaml` 和 `src-tauri/Cargo.lock` 应随项目提交。正式分发前需要配置自己的应用标识、代码签名与 macOS 公证；当前 `dev.tokenrefill.desktop` 仅作开发标识。

### 应用图标

平图图标源图是 `src-tauri/icons/source.png`（1024 × 1024，带透明底板外轮廓）。替换源图后运行 `pnpm tauri icon src-tauri/icons/source.png` 重新生成整套图标；页头标志是 `src/assets/logo.png`，浏览器标签图标位于 `public/`。

macOS 26+ 的自适应分层图标（浅色 / 深色 / 有色 / 降低透明度跟随系统）源文件在 `src-tauri/icons/macos/TokenRefill.icon`，由 `src-tauri/tauri.macos.conf.json` 追加进 `bundle.icon`，只影响 macOS 打包：

- 打包器把 `.icon` 交给 `actool` 编成 `Contents/Resources/Assets.car`，再用 `assetutil` 取回内部图标名写入 `CFBundleIconName`；`CFBundleIconFile`（ICNS）保留，供旧系统回退。
- 缺 `actool`（只装了 Command Line Tools）或 actool 主版本低于 26 时，只记一条日志并跳过 Assets.car，产物仍是平图，构建不失败。
- 这个 `.icon` 是手工生成的草稿：先用 Icon Composer 打开、逐外观检查并重新保存，再 `pnpm tauri build --bundles app`。验收看三处——`.app/Contents/Resources/Assets.car` 存在、`Info.plist` 里有 `CFBundleIconName`、`assetutil --info Assets.car` 里能查到该名字的 `Icon Image` rendition。
- `pnpm tauri dev` 的 Dock 图标是编译期内嵌 ICNS 的单张图，不体现自适应效果；必须用打包后的 `.app` 在 Finder 与 Dock 上验收。

## 目录

```text
src/
  App.tsx             管理窗口外壳与分区导航
  App.module.css      界面组件样式
  styles.css          系统字体、主题 token、全局样式
  lib/desktop.ts      带类型的 Rust 状态与命令入口
  lib/store.tsx       统一状态订阅、刷新与偏好保存
  lib/prefs.ts        主题与默认偏好
  panels/             总览、账户、设置
  assets/logo.png     页头品牌标志
  main.tsx            React 与 Motion 入口
src-tauri/
  src/lib.rs          Tauri 启动入口与 IPC 命令
  src/state.rs        状态模型、JSON 持久化与刷新调度
  src/codex.rs        Codex app-server 只读适配器
  src/main.rs         桌面可执行入口
  capabilities/       本地窗口与事件权限配置
  icons/              应用图标与 1024 源图
  icons/macos/        macOS 分层图标草稿 TokenRefill.icon
  tauri.conf.json     窗口、构建和 CSP 配置
  tauri.macos.conf.json  macOS 专属打包配置（追加分层图标）
public/               浏览器标签图标、Web manifest
```

## 发布

打包由 [.github/workflows/release.yml](./.github/workflows/release.yml) 在 GitHub Actions 上完成：macOS 通用包（Apple Silicon + Intel 的 `.dmg`），以及 Windows x64 的免安装单文件 exe 与 NSIS 安装器（`-setup.exe`）。Windows 不产出 MSI。

发版步骤：

1. 把三处版本号改成同一个值，并重新生成 `Cargo.lock`：

   ```sh
   # package.json / src-tauri/tauri.conf.json 手动改
   cargo check --manifest-path src-tauri/Cargo.toml   # 同步 Cargo.lock
   ```

2. 提交后打 tag 并推送：

   ```sh
   git tag v0.2.0
   git push origin v0.2.0
   ```

CI 会先在 macOS runner 上跑检查（`pnpm build`、`cargo fmt --check`、`clippy -D warnings`、`cargo test`），随后并行构建 macOS 通用包与 Windows x64 产物：macOS 用 `cargo tauri` + ad-hoc 签名；Windows 用 MSVC 工具链，先跑 `pnpm test:rust`，再用 `--no-bundle` 产出免安装 exe，最后用 `--bundles nsis` 产出安装器。两类产物都挂到同一个 **draft Release**，确认无误后手动发布。tag 与 `tauri.conf.json` / `package.json` / `Cargo.toml` 任一处版本不一致时构建直接失败，避免出现版本号对不上的安装包。

也可以在 Actions 页面手动触发 `release`，此时按当前 `tauri.conf.json` 版本创建 `vX.Y.Z` 草稿。

### 签名状态

当前未接入 Apple Developer 证书，产物是 ad-hoc 签名、未公证，其他 Mac 首次打开会提示"无法验证开发者"。可以右键 →「打开」，或先移除隔离属性：

```sh
xattr -dr com.apple.quarantine "/Applications/Token Refill.app"
```

接入证书后，在仓库 Secrets 配置 `APPLE_CERTIFICATE`、`APPLE_CERTIFICATE_PASSWORD`、`APPLE_SIGNING_IDENTITY`、`APPLE_ID`、`APPLE_PASSWORD`、`APPLE_TEAM_ID`，tauri-action 会自动签名并公证；同时删除 workflow 里的 `APPLE_SIGNING_IDENTITY: "-"`。

macOS 26 的自适应图标依赖 runner 上的 Xcode 26 `actool`；runner 版本更低时会按上文跳过 `Assets.car`，产物退回平图图标，构建不会失败。

Windows 的免安装 exe 与 NSIS 安装器由 `build-windows` job 用 MSVC 工具链构建。两者都未配置 Authenticode 证书，首次运行会出现 SmartScreen 提示；免安装 exe 依赖系统 WebView2（Win10/11 通常已预装）。接入代码签名证书后可在该 job 补充签名步骤。Windows 包的安装与卸载验收仍未完成。

## 当前范围

已包含基础窗口、系统浅深色、减少动态效果支持、Motion 反馈动画，以及首个真实业务链路：**Codex 订阅额度查询**。

桌面应用启动后先读取 Rust 侧的缓存快照，再按设置自动刷新：默认每 5 分钟一次，可在设置中调整；支持手动刷新，同一账户的在途请求会去重。查询在 Rust 后台线程执行，窗口切换不会重复发起；失败时保留上一次成功数据并标明错误，不把失败显示成零额度。账户、套餐、额度窗口、更新时间与查询状态由统一状态模型管理，偏好保存到应用数据目录的版本化 JSON。

当前 Codex 查询依赖本机已安装的 Codex CLI。本机账户沿用本机已有登录态；独立账户通过官方授权登录，各自使用隔离的 `CODEX_HOME`，支持添加、改名、重新授权与移除。账户页不再只读。

开机自启已接入系统登录项：默认关闭，开关只反映系统实际状态；写入失败会如实报错且不改本地状态，启动时以系统登录项为准回写偏好。

低额度、授权失效与重置提醒已接入系统通知：低额度在剩余比例低于设置阈值时发送，同一重置周期只提醒一次；授权失效只在状态从正常跃迁到失效时提醒一次，重复查询与应用重启都不会重复；重置提醒默认关闭，只有查询结果同时确认旧周期结束和新周期开始时才发送，不根据本地倒计时推断。去重记录随状态持久化，设置页提供测试通知入口。系统通知是否展示仍由系统通知设置控制，桌面端没有应用内授权弹窗。

尚未实现：

- 百炼 Token Plan、Coding Plan 接入。
- 独立账户凭证的系统级安全存储（当前依赖 Codex CLI 在隔离目录中维护的授权文件）。
- 平台限流退避参数、休眠唤醒后的补偿策略。
- 菜单栏常驻、弹窗定位和窗口焦点策略。
- 原生 Liquid Glass 与旧系统材质回退。目前没有将 CSS 毛玻璃伪称为系统 Liquid Glass。

前端不应持有账户密钥或在日志中输出凭证。后续平台请求和凭证访问优先放在 Rust / 系统层。

## 许可证

本项目采用 [MIT License](./LICENSE)。
