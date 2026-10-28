# macOS 发布参考：devup

- 核实日期：2026-10-21
- 来源：用户指定的本机 devup 仓库，仅静态只读检查
- 目的：为 Token Refill 的双架构安装包及签名公证方案提供依据
- 范围限制：未执行构建、签名、公证、上传，未检查线上发布记录，未读取证书、私钥、Keychain 或账户凭证

本文中的来源路径均相对于 devup 仓库，不是 Token Refill 内文件。配置事实不等于实际发布或运行验证成功。

## 1. 核实结论

| 项目 | 静态配置事实 | 来源 |
| --- | --- | --- |
| 技术栈 | Rust + Tao + Wry；并非 Tauri 应用 | `Cargo.toml:6-14` |
| macOS 架构 | 分别构建 `aarch64-apple-darwin` 与 `x86_64-apple-darwin`，标记 arm64、x64；不是 Universal 合并包 | `.github/workflows/release.yml:94-117` |
| 构建环境 | 两类目标均使用 `macos-14` runner 标签，按 target 构建；不能由此证明已在两类真实设备上运行验证 | `.github/workflows/release.yml:97-117` |
| 应用及安装包 | 手工组装 `.app`，再制作按版本及架构命名的 `.dmg` | `packaging/macos/bundle.sh:49-63`、`:86-103` |
| 默认签名 | 无正式身份时对应用执行 ad-hoc 签名并做签名验证；DMG 也有 ad-hoc 分支 | `packaging/macos/bundle.sh:65-74`、`:98-103` |
| CI 签名配置 | 当前打包步骤未传入 Developer ID 身份，也未请求公证；正式签名相关配置处于注释说明中 | `.github/workflows/release.yml:119-149` |
| 公证预留 | 脚本提供 `--identity`、`--notarize`，正式身份时有 hardened runtime、notarytool 与 staple 分支；未验证该分支可用 | `packaging/macos/bundle.sh:23-33`、`:65-84` |
| GitHub 发布 | 工作流使用 GitHub Releases，挂载 DMG、应用归档及 Windows 文件 | `.github/workflows/release.yml:158-177` |
| 更新归档 | 同时生成 `.app.tar.gz`，README 将其说明为应用内自升级输入 | `packaging/macos/bundle.sh:88-90`、`README.md:77-83` |
| 兼容声明 | devup 的 Info.plist 写最低 macOS 11.0；不是 Token Refill 的支持结论 | `packaging/macos/Info.plist:25-26` |

## 2. 签名与公证必须区分

- ad-hoc 签名不需要付费 Developer ID 证书，但不提供 Apple 核验的开发者身份，也不等于完成公证。
- devup 的 `codesign --verify` 是代码签名完整性检查，不等于 Gatekeeper 放行、公证通过或安装无警告。
- README 明确说明当前包无付费开发者证书、未公证，并提醒首次安装可能被 Gatekeeper 拦截，见 `README.md:109`。本次未在目标系统复现其安装说明，不能承诺某种首次打开方式在所有 macOS 版本有效。
- 不照搬 README 中移除隔离属性的命令，也不将降低系统安全设置作为 Token Refill 的常规安装条件。
- 正式公证分支当前未在工作流启用；后续需重新检查提交容器、签名与打包顺序、staple 目标、证书及凭证配置，不能把预留代码视作可直接启用的已验证流程。

## 3. 发布触发细节

实际工作流不只是“手动构建或推 tag”：

- `v*` tag 推送可进入发布分支，见 `.github/workflows/release.yml:42-45`。
- 手动触发会发布，必要时自动补 tag，见 `.github/workflows/release.yml:46-54`。
- 匹配触发条件的 main 推送在版本 tag 不存在时会自动创建 tag 并发布，见 `.github/workflows/release.yml:55-63`。
- PR、功能分支构建与发布条件分开判断，见 `.github/workflows/release.yml:3-14`、`:158-160`。

这些是 devup 的配置事实，不是 Token Refill 已接受的自动发布授权。本项目不能仅因参考该仓库，就复制 main 推送自动打 tag、发布的行为。

## 4. Token Refill 的已确认取舍与实现建议

用户已明确接受双架构独立 DMG + ad-hoc 签名、暂不公证，并要求说明安装限制。产品方向已定，具体 Tauri 实现仍待验证：

1. Apple Silicon 与 Intel 分别提供 DMG，不要求首版制作 Universal 包；建议使用可识别版本和架构的命名。
2. 初期采用 ad-hoc 签名、暂不公证，发布说明如实标注状态及安装限制，不把该方案称作已公证或无警告安装。
3. 将 Developer ID 签名与公证保留为后续升级路径，不把证书账号作为需求设计或业务开发的前置条件。
4. 使用本项目 Tauri 2 的构建与打包能力实现，而不是复制 devup 手工组装应用的脚本。
5. 为两个架构分别验证目标二进制、签名完整性、安装启动、账户接入、菜单栏、凭证存储及卸载。交叉构建成功不能替代 Intel 设备上的运行验证。
6. 保留 Token Refill 现有 macOS 12.0 配置作为兼容验证起点，不继承 devup 的 11.0 声明。
7. 不照搬 devup 的应用内更新实现；本项目采用 Tauri 官方 updater（minisign 签名、下载与安装两段确认），发布流程见 README「更新签名密钥」。ad-hoc 签名下更新后权限是否重置仍需实测。
8. 不照搬 devup 的正式签名 entitlements。其文件启用 JIT、可写可执行内存、动态库环境变量及关闭库校验等权限，见 `packaging/macos/entitlements.plist:9-17`；本项目应按实际需要最小化选取并验证。

## 5. 尚未验证

- Token Refill 的具体 Tauri ad-hoc 打包配置及两个架构的实际产物。
- devup 历史发布是否成功，以及当前脚本的可选正式公证分支是否可运行。
- 不同 macOS 版本上从浏览器下载 DMG 后的 Gatekeeper 表现。
- Token Refill 的最低系统版本与双架构实际运行兼容性。
- 用户已接受初期暂不公证并明确告知安装限制；该决定不等于已经完成实际安装或签名验证。

## 6. 对当前文档的影响

- [需求文档](./requirements.md)：确认 Apple Silicon + Intel；记录签名公证参考来源，不虚构已签名公证状态。
- [软件设计](./software-design.md)：以 Tauri 能力适配发布路径，保留正式签名与公证升级边界。
- [开发计划](./development-plan.md)：分别验证两类产物与运行环境；发布任务如实区分初期 ad-hoc 与后续 Developer ID 路径。
