# Token Refill

用于查看 AI 套餐额度的本地桌面应用。目前为初始化骨架，尚未接入任何平台或账户。

## 技术栈

- Tauri 2 + Rust：桌面壳与本地逻辑。
- React + TypeScript + Vite：前端界面。
- Motion：交互动效，遵循系统的减少动态效果设置。
- CSS Variables + CSS Modules：组件样式，默认跟随系统浅色 / 深色模式。
- pnpm：前端依赖管理。

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

`pnpm-lock.yaml` 和 `src-tauri/Cargo.lock` 应随项目提交。正式分发前需要配置自己的应用标识、图标、代码签名与 macOS 公证；当前 `dev.tokenrefill.desktop` 仅作开发标识，图标为占位图标。

## 目录

```text
src/
  App.tsx             启动页与连接检测交互
  App.module.css      启动页组件样式
  styles.css          系统字体、主题 token、全局样式
  lib/desktop.ts      带类型的 Rust 调用入口
  main.tsx            React 与 Motion 入口
src-tauri/
  src/lib.rs          Tauri 启动入口与 get_app_info 命令
  src/main.rs         桌面可执行入口
  capabilities/       本地窗口权限配置
  icons/              占位应用图标
  tauri.conf.json     窗口、构建和 CSP 配置
```

## 当前范围

已包含基础窗口、系统浅深色、减少动态效果支持、Motion 反馈动画，以及一个不访问网络的 Rust IPC 命令。

尚未实现：

- 真实账户登录、额度查询和平台适配。
- Keychain 凭证存储、自动刷新及通知。
- 菜单栏常驻、弹窗定位和窗口焦点策略。
- 原生 Liquid Glass 与旧系统材质回退。目前没有将 CSS 毛玻璃伪称为系统 Liquid Glass。

前端不应持有账户密钥或在日志中输出凭证。后续平台请求和凭证访问优先放在 Rust / 系统层。

## 许可证

本项目采用 [MIT License](./LICENSE)。
