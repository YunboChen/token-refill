# Token Refill

用于查看 AI 套餐额度的本地桌面应用。目前为初始化骨架，尚未接入任何平台或账户。

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

## 环境准备

- Node.js 22.12+（建议使用当前受支持的 LTS 版本）。
- pnpm 12。
- Rust 稳定版，包含 Cargo、rustfmt 和 Clippy。
- macOS：Xcode Command Line Tools，可使用 `xcode-select --install` 安装。
- Windows 后续开发还需要 MSVC 构建工具及 WebView2，参见 [Tauri 环境要求](https://v2.tauri.app/start/prerequisites/)。当前未在 Windows 上验证。

当前 macOS 打包配置最低版本为 12.0，不代表已完成该版本的运行验证。

## 开发

```sh
pnpm install --frozen-lockfile
pnpm tauri dev
```

桌面应用启动后，点击“检查 Rust 连接”，应显示 Rust 返回的应用名称、版本、平台和架构。

只预览网页：

```sh
pnpm dev
```

访问 `http://127.0.0.1:1420`。浏览器预览不会模拟 Rust 返回值，连接检测按钮不可用。

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
  App.tsx             启动页与连接检测交互
  App.module.css      启动页组件样式
  styles.css          系统字体、主题 token、全局样式
  lib/desktop.ts      带类型的 Rust 调用入口
  assets/logo.png     页头品牌标志
  main.tsx            React 与 Motion 入口
src-tauri/
  src/lib.rs          Tauri 启动入口与 get_app_info 命令
  src/main.rs         桌面可执行入口
  capabilities/       本地窗口权限配置
  icons/              应用图标与 1024 源图
  icons/macos/        macOS 分层图标草稿 TokenRefill.icon
  tauri.conf.json     窗口、构建和 CSP 配置
  tauri.macos.conf.json  macOS 专属打包配置（追加分层图标）
public/               浏览器标签图标、Web manifest
```

## 当前范围

已包含基础窗口、系统浅深色、减少动态效果支持、Motion 反馈动画，以及首个真实业务链路：**Codex 订阅额度查询**。桌面应用启动后会自动查询并通过本机官方 `codex app-server` 只读获取额度窗口（已用/剩余比例、重置时间），也可手动刷新；要求本机已安装并登录 Codex CLI（未登录或找不到 CLI 时界面会给出明确错误）。不读取、不保存 Codex 登录凭证。

尚未实现：

- 百炼 Token Plan、Coding Plan 接入。
- 凭证独立存储、多账户管理、自动刷新调度、通知。
- 菜单栏常驻、弹窗定位和窗口焦点策略。
- 原生 Liquid Glass 与旧系统材质回退。目前没有将 CSS 毛玻璃伪称为系统 Liquid Glass。

前端不应持有账户密钥或在日志中输出凭证。后续平台请求和凭证访问优先放在 Rust / 系统层。

## 许可证

本项目采用 [MIT License](./LICENSE)。
