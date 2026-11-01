# Token Refill 开发计划文档

- 文档版本：0.16
- 状态：Codex 本机查询、官方授权多账户、百炼 Token Plan 个人版与 Coding Plan 中国站查询、统一状态层、菜单栏快速查看、桌面悬浮球、Windows 兼容层、版本检查与应用内更新已实现
- 最近更新：2026-11-01
- 关联文档：[需求文档](./requirements.md)、[软件设计文档](./software-design.md)

## 1. 计划原则

- 已确认首批覆盖 Codex 订阅、百炼 Token Plan 与 Coding Plan，不将三类支持降为仅一类。
- 已确认首发 macOS，提供菜单栏快速查看与独立管理窗口；Windows 兼容层已实现源码适配，安装包与运行验收仍属后续阶段。
- 先确认关键展示信息、接入边界与刷新行为，再细化任务和估算。
- 优先验证真实平台接入能力，避免界面完成后才发现关键数据无法获取。
- 开发先交付一条端到端可验证链路，再扩展到三类套餐和双入口；中间里程碑不等同于首版完整交付。
- 现阶段不承诺日期、人天或固定迭代次数；待详细范围、能力验证及投入情况明确后补充。

## 2. 当前状态

| 项目 | 状态 | 说明 |
| --- | --- | --- |
| 初始化工程 | 已存在 | Tauri、React、Rust 与本地 IPC 骨架 |
| 了解仓库现状 | 已完成 | 已核实 README、主要入口及配置 |
| 文档草稿建立 | 已完成 | 需求、软件设计、开发计划及文档索引 |
| 核心问题与产品方向 | 已确认 | 减少在软件与 CLI 之间切换，集中查看三类 AI 套餐 |
| 首发系统与形态 | 已确认 | macOS，菜单栏快速查看 + 独立管理窗口；后续 Windows |
| Windows 兼容层 | 已实现 | CLI 查找、授权链接、数据目录、标题栏按平台适配；Windows 本机构建、单元测试、真实 CLI 握手与启动验证通过，安装包与运行验收未完成，详见 [Windows 兼容设计](./windows-support.md) |
| 版本检查与更新 | 已实现 | 设置页查询 GitHub Releases 最新正式版本；启动时静默检查，运行期间定期复查；安装版可显式下载并重启安装（minisign 校验），免安装 exe 只跳转发布页 |
| 展示信息与多账户 | 已确认 | 额度或比例、各周期、重置时间或倒计时、更新时间与状态都需要；支持多个账户 |
| 接入原则 | 已确认 | 应用独立管理多账户、可选主动导入、CLI 非正式版强制依赖；不包含本机凭证读取操作授权 |
| 刷新、后台运行与提醒 | 已确认 | 默认 5 分钟可调自动刷新 + 单账户/全部手动刷新；缓存与失败反馈；关闭窗口常驻、明确退出、自启可选默认关闭；低额度、授权失效及可选重置提醒 |
| 首版范围收敛 | 已确认 | 历史统计不进入首版；Windows 后续支持 |
| 页面与提醒默认值 | 已确认 | 菜单栏 + 总览/账户/设置；添加、命名、重新授权、移除；低额度默认 20% 可调，低额度及失效提醒默认开启且需通知许可 |
| 首版分发目标 | 已确认 | 开源并提供 macOS 安装包，Apple Silicon 与 Intel 均支持，面向全新用户环境验证 |
| macOS 发布参考 | 已核实 | devup 当前配置为双架构分别构建、ad-hoc 签名、未启用公证；只读核实不代表本项目已构建或已接受所有发布行为 |
| 百炼目标版本 | 已确认 | Token Plan 个人版、Coding Plan 中国站；其他版本与站点不自动承诺兼容 |
| 首版详细范围与验收 | 核心范围已收敛 | 初期签名方案已定；兼容环境、发布渠道与部分异常交互通过技术验证后定案 |
| 平台接入资料核实 | 已完成 | 已核实百炼控制台网关协议、浏览器登录回调及字段语义，记录三类计量差异；详见[平台接入可行性记录](./platform-integration-validation.md) |
| 真实账户接入验证 | 基本完成 | Codex 已返回真实快照（未记录额度值）；百炼两类已通过本机控制台会话完成真实读取。剩余：跨账号隔离、令牌续期与限流待实测 |
| 软件设计 | 内部工程草案已补充 | 已有规范化数据模型、IPC 与事件边界、适配器职责、JSON + Keychain 存储建议、刷新提醒和测试矩阵；不是已实现接口或已验证平台映射 |
| 统一状态层 | 已实现 | Rust 管理账户、套餐、额度快照、查询状态与偏好；JSON 版本化持久化，前端读取脱敏状态并订阅变更事件 |
| 后台刷新调度 | 基础能力已实现 | 默认 5 分钟可调、单账户/全部手动刷新、在途去重、缓存优先、失败保留上次成功数据；平台限流参数与休眠唤醒策略待定 |
| Codex 多账户授权 | 已实现 | 每账户独立 `CODEX_HOME`，官方 `account/login/start` 浏览器授权；支持添加、取消、改名、重新授权、移除；不复制本机默认凭证 |
| 菜单栏常驻 | 已实现 | 菜单栏图标左键打开快速查看面板，右键菜单提供打开主窗口、查看额度与退出；关闭管理窗口只隐藏，应用继续后台刷新；macOS 同时隐藏 Dock 图标与 Cmd+Tab 条目，打开主窗口时恢复；面板按屏幕与鼠标位置定位，失焦自动收起 |
| 桌面悬浮球 | 已实现（macOS 已本机验证） | 设置页默认关闭；60×60 置顶窗口显示当前套餐最低剩余比例，悬停详情、点击切换订阅套餐或中转站；可拖动并按屏幕边缘吸附，位置与选择持久化；macOS 用非激活 NSPanel，Windows 用 `WS_EX_NOACTIVATE` / `WS_EX_TOOLWINDOW`，Windows 实机验收待执行 |
| 业务实现 | 进行中 | Codex 本机与独立账户链路已跑通；百炼控制台授权、Token Plan 与 Coding Plan 查询、多产品展示与刷新已接入；凭证系统级存储与限流退避仍待实现 |

## 3. 候选阶段

以下为建议顺序，不代表已确认的详细排期。接入验证结果可能导致范围与设计调整。

### P0：补齐需求与验收基线

- 已完成：确认统一查看目标、三类接入对象、macOS 首发与双入口形态。
- 已完成：确认额度或比例、各额度周期、重置信息、更新时间与查询状态都需要；支持多个账户。
- 已完成：确认菜单栏快速查看及管理窗口总览、账户、设置的组织，账户支持添加、命名、重新授权与移除；具体视觉和异常交互待设计。
- 已完成：确认独立管理多账户、可选主动导入、CLI 非正式版强制依赖的接入原则；具体查询能力与导入来源在 P1 验证，实际读取权限单独授权。
- 已完成：确认自动及手动刷新、缓存与失败反馈、常驻与退出、自启开关和三类提醒范围；明确首版不做历史统计。
- 已完成：确认低额度阈值默认剩余 20% 可调，低额度与授权失效提醒默认开启且需用户允许通知；通知权限、过期口径与刷新选项仍需设计。
- 已完成：确认首版开源并提供 macOS 安装包，不仅供作者本机使用。
- 已完成：确认 Apple Silicon 与 Intel 均支持，并只读核实用户指定的 devup 发布参考。
- 已完成：确认双架构分别提供 DMG，采用 ad-hoc 签名、暂不公证并说明安装限制；正式签名公证后续升级，不要求现阶段具备开发者账号。
- 待确认或验证：最低 macOS 版本、实际发布渠道及是否有期限；平台接入资料与能力优先核实。
- 已补充范围级验收草案；平台验证后完善具体成功、失败和权限拒绝用例。

交付物：已确认的需求基线、待验证问题清单。

退出条件：用户确认首版详细目标、范围与验收场景。

### P1：验证三类套餐的接入可行性

- 按已确认对象分别验证：Codex 订阅、百炼 Token Plan 个人版、百炼 Coding Plan 中国站；不将团队版接口或国际站能力当成已适用。
- 先完成不接触凭证的资料与公开源码核实，再在另行明确授权后执行真实只读查询。无法取得稳定、允许且可独立管理多账户的来源时，先回到方案决策，不进入正式适配开发。
- 确认各自的额度、窗口、单位和重置时间语义，不默认采用同一数据模型或授权来源。
- 验证失效授权、接口限流、后台查询和敏感数据保护要求。
- 按已确认原则，验证应用独立查询与授权、用户主动导入已有授权的可行性与安装前提；CLI 仅作为验证工具或经用户接受的后备方案，不默认变为正式版强制依赖。
- 验证多个同平台账户的授权与查询隔离；如采用导入，检查授权续期、令牌轮换及源工具不受影响的条件。
- 分别记录必需展示字段是否可得，不将 API Key、可调用模型或存在 CLI 命令视作用量查询已可行。
- 记录能力缺口与可接受的替代方案，必要时回到 P0。

交付物：三类接入验证记录、脱敏响应样例、方案选择与风险结论。

退出条件：三类套餐的必需数据均有允许且可行的获取方式，账户隔离与授权生命周期有明确结论；或用户明确确认调整后的范围。文档中存在 CLI 命令、接口名称或合成测试通过均不等同于真实接入验证通过。

#### P1 的三个验证任务

| 编号 | 目标 | 必须产出 | 不允许的捷径 |
| --- | --- | --- | --- |
| VERIFY-001 | Codex 订阅，当前测试 Plus | 协议 schema 与帧格式已核实（CLI 0.163.0）；垂直链路已实现并有假 app-server 端到端测试；剩余一步：用户本人运行应用，与客户端显示核对已用/剩余口径 | 将本地 app-server 协议冒充公开 HTTP API；改写外部默认账户；从截图推断协议或固定所有套餐窗口；代理会话内读取真实账户数据 |
| VERIFY-002 | Token Plan 个人版 | 个人版适用的查询授权、Credits 与订阅月字段、多账户隔离及必要权限 | 因找到团队席位统计 API 就默认个人版可用；直接要求主账号高权限 AccessKey |
| VERIFY-003 | Coding Plan 中国站 | 中国站查询来源与授权、调用次数及窗口字段、续期与限流条件 | 将推理 API Key 视为用量查询必然可用；通过推理请求探测配额 |

每项先记录资料证据与缺口；真实测试需用户明确授权并选定账户。验收样例应脱敏，若某字段不提供则记录降级选择，不自行删需求。

### P2：完成详细设计与实现拆分

已基于确认需求补充内部模型、IPC、存储及测试草案。此类内部设计可与资料初查并行，但不提前冻结未经 P1 验证的平台映射、登录流程或真实字段。

- 细化已确认的菜单栏、管理窗口与账户接入流程。
- 定义数据结构、IPC、错误、平台适配与两个界面的状态共享方式。
- 设计 macOS 凭证安全存储及已确认的刷新、缓存和桌面行为。
- 将系统相关职责与额度业务分离，为后续 Windows 保留必要边界，不提前实现 Windows 成品。
- 拆分任务、依赖、测试方式和交付顺序，并据此估算。

交付物：可执行的软件设计与任务清单。

退出条件：每条首版需求均能追踪到设计、开发任务与验收方式。

### P3：首条真实业务链路实现

- 已选择 Codex 订阅作为首条链路，并已跑通本机官方 app-server 的只读查询与合成数据端到端测试。
- 已补上 Rust 统一状态层与后台刷新调度：缓存优先、默认 5 分钟可调、手动刷新去重、失败保留上次快照。
- 待完成：用户本人运行并与 Codex 客户端核对已用/剩余口径；百炼两类套餐仍以 P1 验证结论为准。
- 通过自动化验证与真实桌面场景核对结果。

交付物：可运行的首条真实业务链路及验证记录。

退出条件：对应验收标准通过，不以模拟数据代替真实接入结果。此阶段完成不代表三类套餐已全部交付。

### P4：三类套餐、双入口与首版发布验证

- 补齐另两类套餐，完成三类套餐在同一应用内的统一查看。
- 完成菜单栏快速查看与独立管理窗口，验证共享状态与单项失败处理。
- 完成已确认的多账户支持，验证同平台多个账户及账户下多个套餐的数据与授权隔离。
- 验证各额度周期、重置信息、更新时间和查询状态；缺失必需字段时回到需求阶段确认，不静默省略。
- 完成已确认的自动与手动刷新、缓存与错误反馈、后台常驻、明确退出及默认关闭的自启开关。
- 完成低额度与授权失效提醒、默认关闭的可选重置或恢复提醒，验证权限处理、去重及重启行为。
- 历史用量统计不进入首版，不因实现缓存或去重而扩大为历史数据产品。
- 检查凭证保护、数据可信度、桌面交互和约定 macOS 环境的兼容性。
- 按确认的架构与渠道准备 macOS 安装包；落实签名公证策略，检查应用标识、图标、版本及许可证材料。
- 在无作者 CLI、登录态和开发环境的全新安装场景验证三类接入、常驻、通知及卸载说明。
- 补充安装、首次接入、权限与本地数据、已知限制和问题反馈文档；检查源码及产物不含真实账户数据或凭证。
- 构建与准备可以先完成，实际公开源码、上传安装包及发布版本应按后续明确的发布指令执行。
- 工作分支一律从 `main` 切出并 PR 到 `main` 合并，`dev` 只承载日常集成、不作为分支基线。
- 发布只走 `main`：发布相关改动同样从 `main` 切分支并 PR 到 `main`，版本号在 `main` 上提升后推送触发构建；`dev` 里未完成或未验证的功能不随发版进入 `main`，发布后再把 `main` 合回 `dev`。

交付物：可开源的源码及文档、macOS 首版候选安装包、验收记录、版本说明与发布检查清单。

退出条件：全部首版需求含 REQ-011 通过验收；渠道、兼容声明与签名状态如实说明，发布阻塞项已解决或由用户明确接受。

### P5：后续 Windows 支持

- 兼容层已实现：CLI 查找覆盖 `codex.exe` / `.cmd` 与常见安装目录，授权链接改用 `ShellExecuteW`，数据目录沿用 `%APPDATA%`，Windows 主窗口改为自绘标题栏（系统装饰在运行时关闭）。
- 重新确认 Windows 版本、架构及使用范围。
- 适配系统安全存储、托盘、窗口行为和打包安装；完成免安装 exe 与 NSIS 安装器构建及全新环境安装验收。
- 验证三类查询链路在 Windows 的依赖条件与可靠性。

交付物与排期：安装包、运行验收记录与支持声明待后续需求细化，不属于本次 macOS 首发验收。

## 4. 任务追踪

详细开发任务待 P0、P1 完成后补充。以下仅记录需求阶段进度与验证准备，不表示已开始开发。

| 任务编号 | 对应需求 | 交付物 | 前置依赖 | 验证方式 | 状态 |
| --- | --- | --- | --- | --- | --- |
| PLAN-001 | REQ-001、REQ-005、REQ-007 | 产品方向、三类套餐与首发形态记录 | 用户输入 | 用户明确回答并同步文档 | 已完成 |
| PLAN-002 | REQ-001 至 REQ-011 | 首版基线；核心功能、页面、提醒默认值与开源安装包已确认，兼容与发布条件待定 | 进一步需求讨论 | 用户确认 | 进行中 |
| PLAN-003 | REQ-001、REQ-002、REQ-003、REQ-008 | 三类套餐接入验证记录；目标版本已确认，分解为 VERIFY-001 至 VERIFY-003 | 查询方式、操作授权及测试账户明确 | 真实结果核对及脱敏记录 | Codex 本机已返回真实快照；百炼两类已完成真实读取并接入。剩余跨账号隔离、令牌续期与限流验证 |

### 4.1 候选开发任务拆分

以下是依据已确认范围整理的工程建议，尚未批准实施，也未估算工期。平台验证结论可能调整具体方案及先后顺序，不能将任务清单视作已验证接口契约。

| 编号 | 对应需求 | 交付物与关键验收 | 前置依赖 | 状态 |
| --- | --- | --- | --- | --- |
| DEV-001 | REQ-001、REQ-002、REQ-003、REQ-008 | 根据真实数据定义账户、套餐、窗口、快照、错误及脱敏 IPC 契约 | PLAN-003、字段与页面设计 | 已实现：三类套餐的真实字段已映射到统一状态模型，含单位、窗口名、可选绝对值与能力缺口 |
| DEV-002 | REQ-003 | 本应用账户记录、隔离安全凭证、经确认的添加/导入/重新授权/移除；不修改源工具登录 | DEV-001、授权方案确认 | Codex 已实现：独立授权目录 + 官方浏览器授权 + 添加/取消/改名/重新授权/移除；系统级安全存储待后续 |
| DEV-003 | REQ-001、REQ-002、REQ-003 | Codex 订阅适配及多账户查询验证 | DEV-001、DEV-002、Codex 能力验证 | 进行中：本机与独立账户链路已实现；多账户真实授权与隔离仍待用户实测 |
| DEV-004 | REQ-001、REQ-002、REQ-003 | 百炼 Token Plan 个人版适配、订阅月比例与重置时间映射及授权隔离验证 | DEV-001、DEV-002、VERIFY-002 通过 | 已实现：控制台网关读取订阅月已用比例与重置时间；单账号真实读取通过，第二账号隔离待实测 |
| DEV-005 | REQ-001、REQ-002、REQ-003 | 百炼 Coding Plan 中国站适配、调用次数窗口与账户下套餐关联验证 | DEV-001、DEV-002、VERIFY-003 通过 | 已实现：5 小时、周、订阅月三窗口保留调用次数；与 Token Plan 共享一次控制台授权；单账号真实读取通过 |
| DEV-006 | REQ-004、REQ-008 | 统一调度、缓存、单账户/全部刷新、并发去重、限流与错误反馈 | DEV-001、至少一类适配可测、刷新参数设计 | 进行中：基础调度、缓存、去重与失败保留已实现；平台限流退避与休眠策略待定 |
| DEV-007 | REQ-005、REQ-009 | 菜单栏弹窗、管理窗口开闭及共享状态；关闭窗口仍运行、退出停止任务 | 页面方向、DEV-001、DEV-006 | 未开始 |
| DEV-008 | REQ-003、REQ-005、REQ-008 | 管理窗口各区域及账户管理交互；真实状态与失败恢复 | 页面方向、DEV-002、DEV-006 | 进行中：总览支持多账户卡片；账户页已启用添加、改名、重新授权与移除 |
| DEV-009 | REQ-006 | 三类提醒、默认 20% 阈值、默认开启的低额度/失效提醒、默认关闭的重置开关、通知权限及去重；失败缓存不误报 | DEV-006、权限/周期与触发边界设计 | 已实现：三类提醒已接入系统通知；低额度按重置周期、授权失效按独立发送回执、重置按新旧快照确认新周期去重，均跨重启保持；授权失效统一覆盖各订阅平台，失败按 1 分钟指数退避至 1 小时重试，禁用时暂停、授权恢复后清理；schema 保持 v1、仅追加带默认值的授权提醒字段，旧失效账户补尝试一次。macOS 使用 `UNUserNotificationCenter`，启动时申请授权，提交回调未知不算成功；横幅展示由系统决定。macOS/Windows 真实通知与权限切换验收待补 |
| DEV-010 | REQ-010 | 自启选项默认关闭，系统设置失败可见且可恢复 | 系统兼容范围、管理设置入口 | 已实现：macOS 使用 LaunchAgent；写入成功并以系统实际状态确认后才回写偏好，失败返回错误；启动时按系统状态校正本地开关；真实登录项行为待用户实测 |
| DEV-011 | 全部首版需求 | Apple Silicon、Intel 分别完成三类套餐与多账户桌面验收、凭证检查、限流离线及全新环境接入；构建通过不替代运行验证 | DEV-003 至 DEV-010、DEV-012 候选包与文档、两类验证环境 | 未开始 |
| DEV-012 | REQ-007、REQ-011 | 正式标识与图标、双架构独立 DMG、ad-hoc 签名验证并说明未公证限制、安装/卸载/权限/接入/许可证文档及开源材料安全检查 | 已确认方案对应的 Tauri 打包验证、可运行应用与发布环境；本期不以前置 Developer ID 证书阻塞 | 未开始 |
| DEV-013 | REQ-011 | 按明确授权发布源码与安装包、版本说明及校验材料；说明兼容范围、签名状态与已知限制 | DEV-011、DEV-012、明确发布指令 | 未开始 |

三类适配无需固定按照表格顺序实施。可以先选一类串起 DEV-001、DEV-002、DEV-006 与基本界面，验证端到端链路后再补齐，首版仍以全部需求验收为准。

## 5. 当前风险与待定条件

| 项目 | 当前影响 | 应对方向 |
| --- | --- | --- |
| 展示要求已定，实际字段与交互行为未验证或确认 | 无法合理细化实现及排期 | 完成 P0 剩余问题与 P1 字段验证 |
| 三类接入能力未验证 | 可能无法获得期望数据或长期后台查询 | 三类分别验证，不假设 API Key 足够或 CLI 随处可用 |
| 多账户已确认，授权隔离与导入安全性未知 | 可能串号、覆盖源工具登录态或产生续期冲突 | 验证独立授权及导入边界，不通过修改默认 CLI 登录态实现多账户 |
| 已确认初期 ad-hoc、暂不公证，实际安装表现与兼容性未验证 | 包可能出现安装提示或拦截；交叉编译不保证 Intel 可运行 | 验证最低系统版本和两类设备，明确发布渠道；如实说明签名、公证及已验证范围，不降低系统保护 |
| 外部用户不具备作者环境 | CLI、登录态或开发工具依赖可能使安装后无法使用 | 全新环境验证接入与运行，明确前置条件，不把开发机成功当成可分发 |
| 开源材料或构建产物含敏感数据 | 可能暴露真实凭证、账户响应或发布密钥 | 发布前检查源码、样例、日志、截图与产物，密钥不入库且不写日志 |
| 双入口独立刷新 | 可能导致重复请求或不一致显示 | 详细设计时明确统一刷新与共享状态 |
| 开发投入及期限未知 | 无法给出可信日期 | 范围与验证结论明确后再估算 |

## 6. 验证记录

当前完成文档更新及用户指定 devup 仓库的只读发布配置核实，记录见 [macOS 发布参考](./macos-release-reference.md)。未执行其脚本、读取证书或密钥，未验证线上发布记录。

已完成首轮套餐资料初查，见 [平台接入可行性记录](./platform-integration-validation.md)。WebFetch 未能通过目标域名安全访问检查，未获取部分线上全文，也未绕过限制；使用本地原始文档快照和已标注的搜索线索记录未决事项，不能据此宣称线上接口已验证。

2026-10-22 已实现 Rust 统一状态层与后台刷新调度：账户/套餐/快照/查询状态/偏好版本化 JSON 持久化，`get_app_state`、`refresh_usage`、`update_preferences` 命令与 `app-state-changed` 事件；缓存优先、默认 5 分钟可调自动刷新、单账户/全部刷新去重、失败保留上次成功快照。前端总览、账户与设置页改为读取统一状态。

2026-10-22 已实现 Codex 多账户授权：账户增加 `source`（本机/独立授权）与脱敏身份提示；独立账户使用 `codex-profiles/<accountId>/` 作为 `CODEX_HOME`；新增 `begin_account_login`、`cancel_account_login`、`rename_account`、`remove_account`、`open_external_url` 命令与 `account-login-changed` 事件。账户页支持添加、取消、改名、重新授权与移除，总览按账户分别展示额度。未读取或复制本机默认登录凭证。

2026-10-22 修复刷新状态遗留问题：`refreshing` 是运行时标记，应用在刷新途中退出会把它持久化，重启后调度器会永远跳过该账户，界面一直停在“刷新中”且不报错。现在启动加载时清除该标记并让账户立即重试；刷新失败且没有快照时按设置间隔退避，不再每 5 秒重试。本机账户同时读取 `account/read` 显示脱敏邮箱与套餐档位，额度请求失败时只要身份读取成功仍会更新身份。

2026-10-23 已实现开机自启：接入 `tauri-plugin-autostart`，macOS 使用 LaunchAgent；新增 `set_auto_launch` 命令，先写入系统登录项并回读实际状态，确认生效后才保存偏好，失败返回错误且不修改本地开关；普通设置保存不会覆盖自启字段；启动时按系统实际状态校正本地开关。开发模式会注册 `target/debug/token-refill`，测试后需关闭。Rust 单元测试 43 项通过，TypeScript 检查通过。

2026-10-23 已实现重置/恢复提醒：在快照写入前记录各窗口的重置时间，刷新成功后只有同时满足“新重置时间晚于旧值、旧周期在上次查询前已到期、新周期在本次查询后仍有效”才发送系统通知，因此不会因本地倒计时或重置时间的小幅前移误报；缺少重置时间或没有上一份快照时不提醒。去重依靠持久化的新旧快照比较，重复刷新与重启不重复。默认关闭。Rust 单元测试 42 项通过，TypeScript 检查通过。

2026-10-23 已实现授权失效提醒：仅在账户 `authState` 从非失效跃迁到 `needs-reauth` 时发送系统通知，普通网络错误不触发；去重直接复用持久化的 `authState`，重复失败与重启不重复，重新授权成功回到 `ok` 后再次失效会重新提醒。通知文案按账户来源区分：本机账户提示重新登录 Codex CLI，独立授权账户提示在应用内重新授权。Rust 单元测试 37 项通过，TypeScript 检查通过。

2026-10-23 已实现低额度提醒：接入 `tauri-plugin-notification` 发送系统通知；刷新成功后按账户/套餐/窗口比较剩余比例与阈值，同一重置周期只提醒一次，去重记录随状态 JSON 持久化，旧状态缺少该字段时回退为空记录；平台未提供重置时间时改用跌破阈值的边沿去重，剩余比例缺失不触发。设置页新增“发送测试通知”入口并更新状态说明。Rust 单元测试 33 项通过，TypeScript 检查通过；真实系统通知展示仍需用户在 macOS 上实测。

本轮验证：Rust 单元测试 28 项通过，TypeScript 检查与前端生产构建通过；使用临时 `CODEX_HOME` 对真实 Codex CLI 0.160.1 验证了 `account/login/start` 返回授权 URL 与 `account/login/cancel`，未打开浏览器、未完成任何真实账户授权。另以真实本机登录态验证 `account/read` 能返回脱敏身份与套餐档位，未记录邮箱原文。`cargo clippy` 因本机未安装该组件未执行，不能记为已通过。真实多账户授权、数据隔离与额度口径仍需用户本人实测。

2026-10-23 已实现 Windows 兼容层：Codex CLI 查找按平台区分 `codex` / `codex.exe` / `.cmd`，补充 npm、pnpm、Volta、Bun、Scoop、nvm-windows、fnm 与官方安装包目录；授权链接改用 `ShellExecuteW`，避免 `cmd /C start` 解析 URL 中的 `&`；数据目录沿用 `app_data_dir()`，Unix 权限位不变，Windows 依赖 `%APPDATA%` 的按用户 ACL；窗口使用系统标题栏，前端按 `get_app_info().platform` 收窄顶部留白。

本轮验证：Windows 10.0.26300 x64 + MSVC 目标，`cargo check --all-targets` 通过，`cargo test` 30 项通过（两个 Python 假 app-server 用例按平台跳过），`tsc --noEmit` 与 `vite build` 通过；真实 `codex.exe` 完成 `initialize` / `account/read` 握手，未登录时返回 `auth-expired`；应用二进制启动后生成 `%APPDATA%\dev.tokenrefill.desktop\state.json` 并写入查询状态；Windows 标题栏留白规则在 Chromium 中验证为 44px → 10px。`cargo clippy` 仍有 1 条既有告警（`apply_login_success` 参数个数），与本次改动无关。MSI/NSIS 安装包、已登录账户查询、实际窗口截图与 Windows 10 具体版本仍未验证；本机 GNU 目标因缺 `dlltool.exe` 无法编译，结论仅对 MSVC 目标成立。

2026-10-23 Windows 产物调整为免安装单文件 exe（`--no-bundle`）与 NSIS 安装器（`--bundles nsis`），不再产出 MSI。本机验证：免安装 exe 10.1 MiB、可正常启动；NSIS 安装器 2.48 MiB。CI 的 `build-windows` job 用 `tauri-action` 创建 draft Release 并上传安装器，再用 `softprops/action-gh-release@v3` 追加免安装 exe；workflow 通过 actionlint，尚未在 GitHub Actions 实际运行。安装、卸载与升级验收仍未完成。

2026-10-23 实现版本检查：新增 `check_for_update` 命令（Rust 侧用 ureq + rustls 请求 GitHub Releases `/releases/latest`，8 秒超时，semver 比较），设置页新增“版本”区块，自动查询并在有新版本时提供“前往下载”；只检查，不下载、不安装。验证：5 项单元测试覆盖版本比较、预发布/草稿与异常 tag；真实接口在无正式版本时返回 404 并映射为“发布仓库暂无正式版本”；临时指向其他公开仓库验证了“发现新版本”界面与按钮（已还原）。`cargo test` 52 项通过，`tsc --noEmit` 与 `vite build` 通过。

2026-10-25 拆分中转站功能：总览新增「订阅账户额度 / 中转站账户额度」标签栏，中转站额度沿用单列额度卡并支持单独刷新；原“中转站”页收敛为“中转站账户管理”，只负责添加、扫描、刷新、补充访问令牌与移除。新增 `RelayProvider` 在总览与管理页之间共享账户状态，首次进入任一入口时才扫描本地配置，避免应用启动时额外查询，也避免两个页面各自扫描导致状态不同步。验证：`pnpm typecheck` 与 `pnpm build` 通过；在运行中的桌面应用内切换两个总览标签页与管理页，确认账户、额度与凭证状态一致。

2026-10-25 重做中转站账户管理页：由表单卡 + 单列账户卡改为与账户页一致的分组标题、账户网格与虚线添加入口；添加、补充访问令牌与移除统一改用弹窗，未识别的本地配置默认收起，首屏不再堆叠说明文字。管理卡只保留凭证/访问令牌状态与额度同步状态，金额、窗口与到期时间仍留在总览。`RelayProvider` 增加 `clearError`，打开弹窗前清理页面级错误。验证：`pnpm typecheck` 与 `pnpm build` 通过；在运行中的桌面应用内检查深色/浅色外观、添加/访问令牌/移除弹窗与未识别配置展开。

2026-10-26 优化 Codex 本机额度获取：桌面端（ChatGPT.app / Codex 桌面应用）内置的 CLI 纳入查找链，排在用户显式安装的 CLI 之后；同时接受桌面端使用的 `CODEX_CLI_PATH`（指向不可执行文件时静默忽略，不阻断常规查找）。macOS 探测 `/Applications` 与 `~/Applications` 下 `ChatGPT.app` / `Codex.app` 的 `Contents/Resources/codex-cli/bin`，并兜底 `CodexCLI.app/Contents/MacOS`；Windows 探测安装版的 `resources\codex-cli\bin` 与 Microsoft Store 版的 `WindowsApps\OpenAI.Codex_*`（枚举受 ACL 限制时跳过）。桌面端与独立 CLI 共用 `~/.codex` 登录态，查询仍只走官方 `app-server --stdio`，不解析桌面端内部状态文件。验证：macOS 上 ChatGPT.app 内置的 codex 0.164.0-alpha.11.1 完成 `initialize` / `account/read` / `account/rateLimits/read`，未记录邮箱、令牌或额度原文；`cargo test --locked` 与 `cargo clippy --all-targets -- -D warnings` 通过。Windows 桌面端路径与 Store 版 ACL 情况尚未在 Windows 机器实测。

2026-10-26 本机 Codex 账户按客户端可用性显隐：应用启动加载状态时检查本机是否存在 Codex 客户端（独立 CLI 或桌面端内置 CLI），不存在则移除「本机 Codex」占位账户，存在但状态里缺少时补回。未安装 Codex 的用户不会再看到一张必然失败的卡片，调度器也不会反复查询；卸载后重新安装会在下次启动恢复。客户端存在但未登录时仍保留账户并展示授权错误，避免把需要登录的账户静默隐藏。验证：`cargo test --locked` 161 项通过，新增 2 项覆盖显隐与快照保留；`cargo clippy --all-targets -- -D warnings` 与 `cargo fmt --check` 通过。未在无 Codex 的全新环境做界面实测。

2026-10-26 新增测试构建工作流 `.github/workflows/build-test.yml`：手动触发、可选择分支与平台，沿用 release 的检查与打包步骤，产物只作为 workflow artifact 保留 14 天，不创建 Release、不打 tag、不改版本号，验证分支不必先合入 `main`。与 `release.yml` 保持两份步骤，改动时需同步。验证：YAML 结构解析通过；workflow 尚未在 GitHub Actions 实际运行。

2026-10-26 版本检查改为运行期间持续复查：此前只在启动时静默检查一次，并复用同一会话的首次结果，应用开机自启后长时间驻留时会错过之后发布的新版本。现在启动后仍先静默检查一次，之后每 6 小时复查；窗口重新获得焦点时若距上次检查超过 1 小时也会复查；检查失败（离线、限流）后 30 分钟静默重试，成功与失败都不打扰用户，设置页仍可手动重试。复查强制绕过会话缓存，设置页重复进入仍复用已有结果。验证：`pnpm build`（`tsc --noEmit` + `vite build`）通过；未在长时间驻留场景做界面实测。

2026-10-26 实现应用内更新（两段式）：Rust 侧接入 `tauri-plugin-updater`，新增 `download_update`、`downloaded_update_version`、`install_update` 三个命令；下载完成并通过 minisign 校验后暂存在内存，用户再次确认才安装。下载进度通过 `update-download-progress` 事件推送并做节流；更新器显式沿应用代理设置，直连模式下调用 `no_proxy()`。设置页“版本”区块增加进度条、“重启并安装”和免安装版说明，侧栏更新入口改为进入设置页版本区块。发布侧：`createUpdaterArtifacts` 只在 CI 用 `--config` 打开，本地构建不需要私钥；macOS 产出 `.app.tar.gz`，Windows 直接以 NSIS 安装器作为更新包并产出 `.exe.sig`，`publish` job 生成并按平台填写 `latest.json`。Windows 通过注册表 `Uninstall\Token Refill` 的 `InstallLocation` 与当前 exe 目录判断是否为 NSIS 安装版，免安装 exe 只跳转发布页。验证：`pnpm build`、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked` 通过。2026-10-26 已配置 minisign 密钥并发布 v0.2.3：三个更新包的签名均用配置中的公钥校验通过；应用内下载与替换仍待 v0.2.4 实测，Windows 注册表判断与 NSIS 静默安装需在 Windows 机器验证。

2026-10-26 macOS 系统通知迁移到 `UNUserNotificationCenter`：此前通知走 `tauri-plugin-notification → notify-rust → mac-notification-sys`，底层是已废弃的 `NSUserNotification`；应用位于前台时系统只把通知写入通知中心、不弹横幅，设置页的测试入口因此永远看不到横幅。现在 macOS 侧新增 `src-tauri/src/notification/macos.rs`，启动时申请通知授权，注册实现 `willPresentNotification` 的 delegate 返回 `banner`，前台也展示横幅；权限被拒绝或尚未决定时返回可直接展示的错误文案，设置页会显示具体原因。Windows 与未打包的 macOS 开发运行仍沿用插件实现。验证：`pnpm build`、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked` 通过；用替换二进制后的临时 `.app` 在 macOS 27 实机验证，日志确认前台发送时出现 `Presenting ... as banner`，随后恢复原安装并清理临时文件。ad-hoc 签名更新后是否需要重新授权，仍待在真实 Release 上验证。

2026-10-26 发布流程加固：新增 `.github/workflows/ci.yml`，在 PR 与推送到 `main` / `dev` 时跑与 release 相同的检查（macOS 上 `pnpm build`、`cargo fmt --check`、`clippy -D warnings`、`cargo test`，Windows 上 `pnpm test:rust`），`main` 分支保护要求 `ci / check` 与 `ci / check (windows)` 通过，不强制 review；`release` 的权限收窄为顶层只读、仅 `publish` 授予 `contents: write`；手动触发限制在 `main`（其他分支直接报错，测试构建走 `build-test`）；`prepare` 增加 semver 格式校验与版本单调性检查，底线取「推送前版本」与「最新 `v*` tag」中较大者，拒绝低于已发布版本的版本号；新增 `.github/dependabot.yml`（actions / npm / cargo 每周检查，次要与补丁合并成一个 PR），并开启 secret scanning、push protection 与 Dependabot 安全更新；为 `v*` tag 建立规则集，禁止删除与移动。验证：本地 actionlint 1.7.12（含 shellcheck）通过，`prepare` 脚本在五种触发场景下按预期构建、跳过或报错；分支保护、tag 规则集与安全设置已通过 GitHub API 确认生效。新 CI 与分支保护需在下次推送后实际跑通一次才能算验证完成。

2026-10-27 中转站扫描改为手动选择并接入 cc-switch：`discovery.rs` 新增只读读取 `~/.cc-switch/cc-switch.db`（SQLite，`providers` 表 `app_type=claude/codex`），Claude 行解析 `settings_config.env`，Codex 行解析 `settings_config.auth.OPENAI_API_KEY` 与 `config` TOML 中激活的 `[model_providers.<id>]`；仍保留 `~/.claude/settings.json` 与 `~/.codex/config.toml` 当前配置，按地址 + 密钥去重。后端由 `relay_discover` 拆成 `relay_scan`（只列候选、不联网）与 `relay_import`（用户勾选后查询额度），候选密钥留在 Rust 内存；`RelayOrigin` 新增 `cc-switch`，账户卡可显示 provider 名称。前端 `RelayProvider` 进入页面时只加载已保存条目，管理页新增「扫描本地配置」弹窗，默认勾选尚未添加的候选，确认后导入。依赖新增 `rusqlite`（`bundled`，只读打开，不依赖系统 sqlite3）。验证：`cargo test`（新增 cc-switch 解析、数据库读取、候选去重与只导入选中项等用例）、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`pnpm build` 通过；尚未在真实桌面窗口内走完整扫描/导入交互。

2026-10-27 补充：扫描导入的账户改为跨重启保留，但仍不复制密钥。`RelayStore` 在 `relay-profiles/<relayId>/source.json` 保存来源引用（来源、app_type、provider id、导入地址、provider 类型与用户主动填写的访问令牌），启动时只有存在引用才以只读方式重新读取 cc-switch 数据库或当前配置，取不到就跳过该账户；额度快照继续写入 `snapshot.json`。`SourceRef` 按 provider id 匹配，Claude Code 当前配置没有 id 时按地址匹配。手动再次添加同地址同密钥的账户会把该条目转为 `credentials.json` 并删除 `source.json`。前端账户卡区分「凭证已保存 / 来源已保存 · 启动时重新读取 / 凭证仅本次运行」。验证：新增 4 项测试覆盖引用落盘不含密钥、重启恢复、来源消失后跳过、手动条目转存；`cargo test --locked` 177 项通过，`cargo fmt --check` 与 `cargo clippy --locked --all-targets -- -D warnings` 通过。

2026-10-28 拆分测试版与正式版渠道：新增 `src-tauri/tauri.test.conf.json`，测试包通过 `--config` 覆盖 `productName`（Token Refill Test）与 `identifier`（`dev.tokenrefill.desktop.test`），数据目录、单实例锁、WebView2 用户数据、通知权限与开机自启随之隔离，可与正式版同时安装、同时常驻。`build-test` 在构建时注入 `<基础版本>-test.g<短提交>` 版本（`g` 前缀规避纯数字短提交的前导零问题），并写入纯数字 `CFBundleVersion`；`meta` job 校验测试配置的 identifier / productName，`release.yml` 拒绝测试 identifier 与带 prerelease 的正式版本号。Rust 新增 `channel.rs`，按 identifier 的 `.test` 后缀判定渠道；`get_app_info` 返回合并配置中的名称、版本与 `channel`，窗口标题、托盘菜单 / tooltip、通知与重新授权文案统一改用应用名；测试渠道的 `check_for_update` 不请求 GitHub，`download_update` / `install_update` 直接拒绝，Windows 卸载键按 productName 动态生成。macOS 测试版使用带右下角圆点的模板托盘图标；新增 `pnpm tauri:dev:test`，本地开发也走测试标识。前端由 `AppStateProvider` 统一读取 `get_app_info`，三处版本展示不再依赖 package.json，测试版侧栏与设置页显示测试版本和“测试版”标记，并隐藏更新入口。

本轮验证：`pnpm build`、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`（186 项单元测试 + 12 项集成测试）通过；本地按 CI 方式注入版本构建 `Token Refill Test.app`，`Info.plist` 的 `CFBundleIdentifier` 为 `dev.tokenrefill.desktop.test`、`CFBundleShortVersionString` 为 `0.2.6-test.g0dd074f`、`CFBundleVersion` 为 `0.2.6`；测试版与正在运行的正式版调试实例可同时启动，数据写入 `~/Library/Application Support/dev.tokenrefill.desktop.test/state.json`（0600），侧栏显示 `v0.2.6-test.g0dd074f` 与“测试版”，设置页显示“测试版不检查更新”且无更新按钮，通知文案使用 `Token Refill Test`。`actionlint` 1.7.12 通过（本机未安装 shellcheck，未覆盖其检查项）。随后推送 `feat/test-channel-split` 并触发 `build-test`（run 40659494237）：`meta` / `check` / macOS arm64 / macOS x64 / Windows x64 全部通过，产物为 `test-macos-arm64-0.2.6-test.ga6f823d`、`test-macos-x64-0.2.6-test.ga6f823d` 与 `test-windows-0.2.6-test.ga6f823d`；挂载 CI 产出的 arm64 DMG 确认 `CFBundleIdentifier` / `CFBundleDisplayName` / 两个版本字段正确，Windows 便携 exe 与 NSIS 安装器的版本资源均为 `Token Refill Test` / `0.2.6-test.ga6f823d`。Windows 的 NSIS 安装目录 / 卸载键、双版本实机并存与卸载互不影响仍未验证。

2026-10-28 降低常驻内存：窗口 WebView 按需创建、按空闲销毁。实测 macOS 空闲时 token-refill 34 MB + 主窗口 WebContent 39 MB + GPU 19 MB + Networking 7.5 MB，另有一个启动时就预创建的菜单栏面板 WebContent（约 36 MB）；每次 Codex 查询还会临时拉起 node 启动器与原生 app-server（2–3 秒）。现在 `tray-panel` 在 `tauri.conf.json` 改为 `create: false`，第一次点开时才用 `WebviewWindowBuilder::from_config` 创建；面板与主窗口隐藏 60 秒后由后台线程 `destroy()` 释放 WebView，再次打开时按配置重建。重建窗口先隐藏创建，等 `on_page_load`（Finished）或 1.5 秒兜底后再显示与定位，避免白屏；加载期间的重复点击只更新待显示位置。主窗口在前端持有登录弹窗或更新下载进度时不销毁（`StateStore::has_active_login` / `UpdateStore::is_downloading`）。面板前端在挂载时视为首次打开，补一次状态同步与中转站过期检查，避免错过加载前的事件。销毁最后一个窗口会触发 `ExitRequested`，在 `RunEvent` 里按“没有窗口且不是程序化退出”拦截，避免菜单栏应用被系统退出。验证：`cargo test --locked`（185 项，新增空闲截止时间与 `create: false` 配置用例）、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`pnpm build` 通过；macOS 实机：启动后只有 1 个 WebContent，首次点开面板 208 ms，隐藏 60 秒后面板 WebContent 退出；主窗口关闭 60 秒后 WebView 退出，进程保持存活且 footprint 回落到 31 MB；再次启动应用重建主窗口耗时 235–320 ms，重建后窗口与额度正常显示。面板销毁后的再次创建与主窗口空闲销毁调用同一套创建函数，未单独复测。Windows 侧待平台内验收，步骤与记录表见 [Windows 内存优化验证](./windows-memory-validation.md)。

2026-10-28 标题栏按平台分治：macOS 恢复系统 Overlay 原生标题栏（`titleBarStyle: "Overlay"` + `hiddenTitle`、侧栏 44px 红绿灯安全区），不再渲染自绘栏；Windows 保留自绘 `TitleBar`，在 Rust `setup()` 中用 `#[cfg(windows)]` 调 `set_decorations(false)` 关闭系统装饰。未使用 `tauri.windows.conf.json` 覆盖窗口，因为平台配置走 JSON Merge Patch，`app.windows` 数组会被整体替换。前端首帧用 UA 同步判断平台，避免 Windows 标题栏闪烁。验证：macOS 实机启动确认原生红绿灯与 44px 安全区恢复、自绘栏不渲染；`pnpm build`、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings` 通过。Windows 运行时关闭装饰的无闪烁表现、拖拽/边缘缩放/双击最大化/关闭常驻与贴靠布局仍需 Windows 实机验证。

2026-10-28 修复 Windows 启动时露出系统标题栏：实机验收「冷启动无原生标题栏闪烁」一项不通过，逐帧录像显示启动后约 1 秒内仍是系统标题栏，React 首绘后才切成自绘栏——`setup()` 里 `set_decorations(false)` 执行时窗口已可见。主窗口改为在 `tauri.conf.json` 中以 `visible: false` 创建：macOS 在 `setup()` 中直接 `show()`（行为不变）；Windows 先关装饰，等前端 `TitleBar` 首帧 emit `main-ui-ready` 后再显示并聚焦，另起 5 秒兜底线程，避免前端异常时窗口一直隐藏只能从托盘找回。验证：`tsc` 类型检查与 `vite build`、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`（macOS 主机）、`cargo check --target x86_64-pc-windows-msvc` 通过；Windows 实机需重跑冷启动检查，并确认隐藏启动不影响托盘、单实例与最小化恢复。

2026-10-29 修复 Windows「放置一段时间后托盘菜单与主窗口都打不开」：上一轮的窗口空闲销毁把应用带进“零 WebView”状态，再创建窗口时是 WebView2 冷启动，长度可达数百毫秒到数秒，而创建被放在托盘/窗口事件回调里就地执行（`run_on_main_thread` 在主线程会内联执行），期间消息循环被占用、重入的点击还会走进创建分支。现在 `show_panel` / `show_main_window` 先转到后台线程再回主线程，事件回调立即返回；`PANEL_LOADING` / `MAIN_LOADING` 提前到创建入口判断，创建期间再点一次左键视为取消，不再重复创建（重复创建会自增代次，让真正在加载的那一代永远等不到显示回调）。面板失焦收起改为必须“确实获得过焦点且距上次显示超过 400 ms”，未拿到激活态时补一次聚焦，补焦失败也不隐藏；兜底显示后再补一次可见性重试，重试受“期望可见”标志约束，用户手动收起后不会被重新弹出。新增 `logging` 模块，把 `[app]` / `[tray]` / `[window]` 生命周期事件同时写入 `<数据目录>/logs/window.log`（256 KiB 轮转，不含凭证）——GUI 子系统的发布包没有控制台，原来的 `eprintln!` 在 Windows 上全部被丢弃。验证：`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`（190 项单元测试 + 12 项集成测试，新增失焦判定用例）、`pnpm build` 通过；macOS 测试渠道实机冒烟：启动后主窗口 `visible=true`，左键打开面板创建耗时 77–208 ms，失焦收起后 60 秒销毁成功，销毁后再次点开可正常重建，日志文件按预期写入。Windows 实机仍需按 [Windows 内存优化验证](./windows-memory-validation.md) 的 W-17 复现验证。

2026-11-01 新增桌面悬浮球（默认关闭）：设置页「通用」新增开关，开启后创建 60×60 的 `floating-ball` 置顶透明窗口与按需复用的 `float-popover` 弹层；两者都复用同一份前端产物，通过 `?view=float` / `?view=float-popover` 区分。状态层新增 `Preferences.floating_ball_enabled` 与 `PersistedState.floating_ball`（当前套餐键 + 物理位置 + 显示器名称），旧状态文件由 serde 默认值补齐，不升 `schemaVersion`；位置与选择随状态文件持久化，重启恢复。球内显示订阅套餐或中转站账户的最低剩余比例与周期，金额型中转站没有可靠比例时显示剩余金额；悬停显示详情卡，点击打开套餐切换菜单，再次点击/失焦/Esc 收起。拖动由前端指针事件触发，Rust 轮询光标移动窗口，按完整屏幕边缘吸附（不是工作区），吸附后保留 24 点可见，悬停展开到 12 点间距，移出 1 秒收回。macOS 上球与弹层转换为非激活 `NSPanel`，球不取 key 焦点、菜单取 key 以接收 Esc 与失焦；Windows 上两者加 `WS_EX_TOOLWINDOW`，球再加 `WS_EX_NOACTIVATE`。新增 `capabilities/floating.json`，只授予 `core:default` 与 `core:event:default`。修复了全局 `body { min-width: 320px }` 把 60px 球窗口内容撑宽、球被居中到窗口外的问题。验证：`pnpm build`、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`（230 项单元测试 + 12 项集成测试，新增悬浮球默认值、旧状态兼容、位置往返与吸附几何用例）通过；macOS 测试渠道实机确认设置开关、60×60 球显示、悬停详情、点击菜单、选择持久化、拖动到左边缘吸附（x=-36，保留 24 点）与 Esc 收起正常。Windows 目标本机缺少 MSVC SDK 头文件，未能完成交叉 `cargo check`，Windows 实机未验证；中转站金额型显示与多显示器/不同缩放场景未用真实账户逐项验证。

2026-11-01 修复多显示器下的悬浮球吸附越界：原先统一按“屏幕边缘保留 24 点可见”计算，右屏左边缘或左屏右边缘这类内部边界会把球推出当前屏幕，露出的部分直接显示在相邻显示器上。现在 `monitor_has_neighbor` 检查“球被藏起来的那一段”是否与任意其他显示器相交（按球的垂直范围计算，覆盖左右排列、上下错位与留缝）；虚拟桌面最外侧边缘继续把 60 点窗口移出屏幕，内部边界则把窗口缩到 24×60 点，由前端按 `floating-ball-layout` 事件的 `edge` / `clipped` 只绘制球的一侧，仍然保留 D 形收起与悬停展开，但不会跨屏。恢复位置时用球的可见部分探测显示器；运行期间接入/移除显示器时在悬停/收回路径重新判定；悬停展开会优先避免把球从鼠标下移走。验证：`cargo test --locked`（231 项，新增内部边界用例）、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`pnpm build` 通过；macOS 双屏实例实测，主屏右边缘的球从 60×60、x=1896（36 点溢到相邻屏幕）变为 24×60、x=1896（完整在主屏内，只渲染左侧 D 形）。Windows 与其他多屏排列（上下排列、不同缩放、显示器热插拔）尚未实机验证。

2026-11-01 悬浮球收起态按设计稿重排并加入过渡：收起时不再简单裁掉 60 点球的一侧，而是把露出部分重排成 D 形——平台标识移到露出 D 形的视觉中心，百分比与周期淡出，额度环只保留露出侧的弧、把进度重新归一化到这段弧并沿用低额度颜色；布局事件新增 `collapsed` 字段，两种吸附（移出屏幕与缩到 24×60）共用同一套前端状态。Rust 侧新增 `animate_ball`，按 220ms ease-out 逐帧过渡窗口位置与宽度，前端 logo/数值用相同时长和缓动的 CSS 过渡衔接；拖动开始与窗口销毁会取消进行中的动画，弹层改用最近一次布局目标定位，避免跟随动画抖动。悬停展开的贴边判定改为包含边界，防止鼠标停在 12 点间隙边缘时反复展开/收起。另加悬停看门狗：非 key 的 NSPanel 在菜单取得焦点后可能收不到 `pointerleave`，每 150ms 核对光标是否还在球或详情卡上，离开超过 1 秒就清掉过期标记并收回，修复菜单操作后球停在展开状态的问题。验证：`pnpm build`、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`（233 项单元测试 + 12 项集成测试，新增收起宽度与缓动用例）通过；macOS 实机吸附态截图确认 logo 居中、额度环只露外弧，窗口宽度采样 24→60 约 184ms 平滑变化；Windows 透明窗口的逐帧缩放与多显示器表现仍需实机验证。

2026-11-01 修复悬浮球悬停不灵敏（需要先点击才响应）：macOS 只向 key 窗口派发鼠标移动/悬停事件，而球与详情卡都是非激活 `NSPanel`，球在点击取得 key 之前收不到前端的 `pointerenter`，表现为“悬停没反应、点一下之后才正常”。悬停看门狗（每 150ms 轮询光标）原来只兜底 `pointerleave` 丢失导致的滞留，现在同时兜底进入：光标进入球或详情卡矩形时按位置补发悬停，未显示/拖动中/菜单打开时跳过；轮询只在“从外到内”的跨越时触发一次，拖动或菜单结束后要求指针先移出再移入，避免松手或松开菜单时立刻重新展开。前端指针事件保留，作为窗口取得 key 后的补充。吸附收起态的详情卡改为等展开过渡结束再显示：`set_hover` 发现球处于“吸附 + 收起”时只展开窗口，另挂起一次显示请求（`hover_show_pending`），220ms 过渡完成后再核对光标仍在完整球体上才调用 `show_popover`；同时到达的 `pointerenter` 与看门狗通知共用同一个请求，指针提前离开、菜单接管或窗口销毁都会取消，自由位置与已展开的球仍然立即显示。验证：`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`（236 项单元测试 + 12 项集成测试，新增悬停进入去重与显示时机用例）通过；macOS 实机 dev 实例重启后，光标尚未点击球就出现 `float-popover 已显示 mode=hover`（修复前 `mode=hover` 总在 `mode=menu` 之后），确认悬停可直接触发详情卡。Windows 上 WebView2 悬停本就无需焦点，同一套轮询同样生效但未实机复测。

2026-11-01 统一主窗口、菜单栏面板与悬浮球的额度颜色层级：新增 `src/lib/quotaLevel.ts`，三处共用同一套等级——剩余高于 70% 为主色（正常），小于等于 70% 且高于提醒阈值为琥珀色（注意），小于等于提醒阈值为红色（危险，含 0%），没有可靠比例为灰色（未知）；关闭低额度提醒时不做警戒着色。主窗口的订阅额度条、中转站比例条与数值，菜单栏的额度行与中转站比例条，悬浮球圆环与弹层额度行全部改为 `data-level`（取代原先混用的 `data-low` / `data-unknown` / `data-severity='muted'`）；0% 的说明改为「额度已用尽」。悬浮球仍保持固定深色 HUD，但颜色改由 `--hud-*` token 提供（值等于深色主题语义色），不再写死 `#4b9cff` / `#ff6b6b` / `#64748b`；中转站弹层额度行此前完全不标 low，现按同一等级着色。三处进度条轨道统一为 `--border`。同步更新 `docs/design.md` 的额度层级、token 表与页面说明。验证：`pnpm build`（`tsc --noEmit` + `vite build`）通过；三处界面实际视觉对比与浅色/深色、阈值调整场景未做截图验收。

2026-11-01 调整悬浮球收起/展开动画：原先窗口位移、宽度收窄与前端 D 形变形在同一段 220ms 里完成，松手点离边缘还有一段距离时球在半路上就已经变成 D 形，看起来像“还没贴边就吸附了”。现在收起拆成两段——`animate_ball_collapse` 先按 160ms 把完整 60 点球体滑到边缘（外侧边缘用带约 7% 回弹的 ease-out，内部边界只用普通缓出，越过贴边位置会露到相邻显示器），贴边后由主线程发出布局事件，再延迟一帧按 180ms 收窄窗口并播放内容变形；布局事件因此不会在滑动途中提前切换 `collapsed`，滑动期间开始悬停/拖动会取消过期段。展开改为 220ms 带回弹的 ease-out，详情卡仍然等这段过渡结束再显示；收起方向的内容过渡换成 180ms 带轻微回弹的曲线，`prefers-reduced-motion` 一并覆盖收起选择器。另修正一处布局假设：圆环与中心内容都是相对 60×60 的 `.ball` 定位，不随窗口/舞台宽度变化，内部边界收起时 logo 仍用 `translate(±15px, 13px)` 移到露出条带的视觉中心（一度按“舞台 24 点宽”给内部边界加了水平居中的覆盖规则，反而把 logo 推到窗口外、收起态看不到平台标识，已删除）。验证：`pnpm build`、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`（悬浮球相关 16 项，新增回弹曲线起止与幅度用例）通过；macOS 双屏实机确认收起态平台标识回到 D 形条带内，动画观感与 Windows 透明窗口未复测。
