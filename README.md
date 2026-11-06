<div align="center">
  <img src="src/assets/logo.png" alt="Token Refill 应用图标" width="96">

  <h1>Token Refill</h1>

  <p><strong>在桌面集中查看 AI 套餐额度、重置时间和中转站余额。</strong></p>

  <p>macOS · Windows · MIT</p>

  <p>
    <a href="https://github.com/YunboChen/token-refill/releases/latest"><strong>下载最新版</strong></a>
    · <a href="#支持的服务">支持的服务</a>
    · <a href="#本地开发">本地开发</a>
  </p>
</div>

## 下载与安装

从 [GitHub Releases](https://github.com/YunboChen/token-refill/releases/latest) 下载与你的系统匹配的文件。

| 系统 | 下载文件 | 安装方式 |
| --- | --- | --- |
| macOS 12+ · Apple Silicon | `*_macos-arm64.dmg` | 打开 DMG，将应用拖入「应用程序」 |
| macOS 12+ · Intel | `*_macos-x64.dmg` | 打开 DMG，将应用拖入「应用程序」 |
| Windows x64 · 安装版 | `*_windows-x64-setup.exe` | 运行 NSIS 安装器 |
| Windows x64 · 便携版 | `*_windows-x64.exe` | 下载后直接运行，系统需有 WebView2 |

macOS 安装包采用 ad-hoc 签名，尚未公证。Windows 安装版与便携版目前均未配置 Authenticode 签名，首次运行可能出现 SmartScreen 提示。

正式版会检查更新。macOS 和 Windows 安装版可在设置中分别确认下载和安装，Windows 便携版通过发布页手动更新。

## 支持的服务

| 服务 | 接入方式 |
| --- | --- |
| Codex&nbsp;订阅 | 通过本机 Codex CLI 或 Codex 桌面端附带的 CLI 查询。可沿用本机登录态，也可通过官方浏览器授权添加独立账户；总览会显示可用的重置卡，展开后可查看明细并在二次确认后使用一张 |
| 阿里云百炼 | 通过官方控制台浏览器授权添加账户，支持 Token Plan 个人版与 Coding Plan 中国站；控制台会话约 24 小时有效且不能续期，应用会在到期前 2 小时提醒重新授权 |
| 腾讯&nbsp;WorkBuddy | 通过官方登录页授权添加账户，按套餐基础 / 赠送 / 购买 / 平台奖励分组读取中国站 Credits 与各自的重置或到期时间；访问令牌过期时自动续期，续期失效后需重新授权 |
| OpenCode&nbsp;Go | API Key |
| Command&nbsp;Code | API Key |
| Google&nbsp;Antigravity | 沿用本机 Antigravity CLI（`agy`）登录态 |
| Cursor | 通过官方 Cursor Agent CLI 在独立目录中登录，读取套餐用量百分比与 Grok Bot 周用量；不读取现有 Cursor 登录、浏览器 Cookie 或 Cursor.app 数据（实验性，额度接口未公开文档） |
| New&nbsp;API&nbsp;/&nbsp;Sub2API | 在「中转站」手动添加，或从 cc-switch、Claude Code、Codex 的现有配置扫描并勾选导入 |

## 开始使用

1. 打开应用，在「账户」或「中转站」添加要查看的服务。
2. 在「总览」查看额度、重置时间和最近刷新状态；有账户需要重新授权、刷新失败或额度已用尽时，右上角会出现「需处理」按钮并标出数量，点一次只看这些账户，再点恢复全部。应用默认每 5 分钟刷新一次，查询失败时会保留上次成功的数据并显示错误。Codex 账户有可用重置卡时，额度区下方会显示数量；打开后可查看每张卡的到期时间，使用前必须再次确认。
3. 从 macOS 菜单栏或 Windows 系统托盘快速查看额度。关闭主窗口后应用继续刷新，需要结束运行时可从菜单栏或托盘退出。
4. 在「设置」中按需开启桌面悬浮球、开机自启，或调整刷新间隔和低额度、授权失效、授权即将到期、额度重置提醒。中转站账户可在「中转站账户管理」里单独设置低余额金额提醒；只有能明确识别余额与币种的站点可开启，余额低于阈值时提醒一次，回升后恢复提醒资格。

## 数据与隐私

额度查询由本机直接向对应服务发起，设置与额度快照保存在本机应用数据目录。项目没有自建后端、云同步或遥测。

Codex 授权由官方 CLI 管理，独立账户使用隔离的 `CODEX_HOME`。用户手动填写的密钥与会话凭证保存在应用私有目录。扫描导入中转站时，应用保存来源引用，并按引用读取原配置中的密钥。

## 本地开发

项目使用 Tauri 2、Rust、React 19、TypeScript 和 Vite。开发需要 Node.js 22.12+、pnpm 12 和稳定版 Rust。macOS 需安装 Xcode Command Line Tools，Windows 需安装 MSVC 构建工具与 WebView2，并使用 `x86_64-pc-windows-msvc` 工具链。

```sh
pnpm install --frozen-lockfile
pnpm tauri:dev:test
```

本地测试版与正式版使用独立数据目录。已安装的 CI 测试包与本地测试版共用数据目录和单实例锁，请分别运行。

```sh
pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings
pnpm test:rust
```

- 项目需求、架构与平台适配见 [文档索引](docs/README.md)。
- 分支约定、版本同步及发布约束见 [AGENTS.md](AGENTS.md)。
- 正式发布与测试构建分别使用 [release](.github/workflows/release.yml) 和 [build-test](.github/workflows/build-test.yml) 工作流。

## 许可证

本项目采用 [MIT 许可证](LICENSE)。
