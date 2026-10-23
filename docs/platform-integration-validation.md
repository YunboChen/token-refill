# 平台接入可行性记录

- 记录日期：2026-10-21；百炼补充验证：2026-10-23
- 阶段：百炼两类套餐已通过本机控制台会话完成真实读取验证（见第 7 节）；Codex 已实现适配器
- 范围：公开搜索结果、本地已安装技术资料与原始文档快照；无账户操作
- 关联：[需求文档](./requirements.md)、[软件设计](./software-design.md)、[开发计划](./development-plan.md)

## 1. 证据与限制

本轮未运行 CLI、读取登录态或凭证、登录平台、调用真实账户接口或发起模型推理。

- **原始资料快照**：已读取本地百炼 skill 的 raw 文档，可作为字段与规则线索，仍需与当前线上版本核对。
- **CLI 命令参考**：已读取安装的 `bailian-cli/reference/usage.md`，其中 Token Plan 与 Coding Plan 查询标为 Console 鉴权；尚未执行命令。
- **公开搜索结果**：找到官方文档、官方源码及协议 schema 的候选入口；搜索摘要不等同于已读完整原文或稳定接口承诺。
- **抓取限制**：本轮 WebFetch 无法通过 help.aliyun.com、github.com 的安全访问检查，未取得对应完整页面；没有改用其他工具绕过限制。未据此宣称接口不存在。
- **资料质量**：Token Plan 的合成 wiki 页面与 raw 个人版文档存在计量及周期冲突，本记录采用 raw 内容，不使用合成页的 `token_quota` 或 `/v1/token-plan` 等未经原始资料支持的说法。质量元数据文件未找到，未虚构评分。

## 2. 三类套餐的初步能力矩阵

| 对象 | 已找到的资料线索 | 额度与时间线索 | 鉴权与接入未决项 | 当前结论 |
| --- | --- | --- | --- | --- |
| Codex 订阅 | 官方仓库 app-server 的 `account/rateLimits/read`、响应 schema 与测试等搜索结果 | 存在比例、窗口及重置时间相关的协议线索；完整字段、精度与账户适用性未逐项核实 | 本地 app-server 不等于公开 HTTP API；独立授权、多账户隔离、是否需要分发额外组件及许可待研究 | 有候选来源，未验证，不能承诺无 CLI 依赖已实现 |
| 百炼 Token Plan 个人版（用户已确认） | 个人版原始概述、FAQ；官方 CLI 查询入口；订阅席位统计 OpenAPI 原始资料 | 个人版以 Credits 计量、按订阅月重置；席位统计资料列有总额、剩余及刷新时间 | 席位统计接口不能默认适用于个人版，不因有此资料就使用团队路径；Console、RAM/OpenAPI 与推理 Key 不等同 | 目标版本已定，适用的独立查询授权及真实结果未验证 |
| 百炼 Coding Plan 中国站（用户已确认） | 原始 FAQ 与官方 CLI 查询入口 | 原始 FAQ 说明按模型调用次数计量，涉及 5 小时、周及订阅月额度，不提供 Token 消耗及特定模型用量 | CLI 文档标为 Console 鉴权；中国站无需 CLI 的允许查询方式、独立登录及多账户方案仍待验证 | 目标站点已定，不能强制显示为 token 数；独立接入未验证 |

## 3. 百炼原始资料核实

本节路径相对于已安装的 `bailian-docs-llm-wiki` skill；不要求应用使用者安装该 skill。

### 3.1 Token Plan 个人版

已读取：

- `raw/model-user-guide/token-plan-guide/token-plan-personal/token-plan-personal-overview.md`：第 3 行、79-104 行。
- `raw/model-user-guide/token-plan-guide/token-plan-personal/token-plan-personal-faq.md`：第 5-35 行、55-69 行、98-118 行。

文档快照明确说明：

- 套餐按 Credits 计量，而非一份可直接换算的固定 token 余额。
- 按订阅月计算与重置，不是固定每月 1 日；个人版当前文档说明无周限额。
- 套餐、补充用量包与 Harness 权益存在不同口径；Harness 权益独立核算，不从套餐 Credits 抵扣。
- 主套餐额度为零时，若另有可用补充包，不应仅据此断言所有调用都不可用。补充包及 Harness 的首版展示范围尚未确认，不能暗中加入或忽略口径差异。

线上来源入口：

- [Token Plan 个人版概述](https://help.aliyun.com/zh/model-studio/token-plan-personal-overview)
- [Token Plan 个人版常见问题](https://help.aliyun.com/zh/model-studio/token-plan-personal-faq)

### 3.2 Token Plan 订阅席位统计 API

已读取：

- `raw/model-api-reference/token-plan-api/token-plan-api-subscription/get-subscription-stats.md`：第 1-27 行、68-127 行。
- `raw/model-api-reference/token-plan-api/token-plan-api-organization/get-token-plan-account-detail.md`：第 5-27 行。

资料列出 `GetSubscriptionStats`，使用阿里云 AccessKey 签名及相应 RAM 权限，不支持用推理 API Key 直接作为 Bearer 认证。响应资料包含订阅开始、结束时间以及席位总 Credits、剩余 Credits、刷新时间等。

这是值得核实的公开 OpenAPI 线索，不能简单断言“百炼没有查询 API”；但其中席位语义明显需要确认，不能据此认定个人版、所有团队账户或 Coding Plan 都可使用。也不因此要求用户提供主账号 AccessKey：后续优先验证最小只读授权、适用范围与更合适的接入方式。

来源入口：[GetSubscriptionStats OpenAPI Explorer](https://api.aliyun.com/api/ModelStudio/2026-02-10/GetSubscriptionStats)。本轮未请求该接口，也未独立抓取其线上全文。

### 3.3 Coding Plan

已读取或搜索到原始文件对应段落：

- `raw/model-user-guide/token-plan-guide/coding-plan-guide/coding-plan-faq.md`：第 96-112 行、681-687 行。

该快照说明额度按模型调用次数消耗，不提供 Token 消耗信息，也不提供套餐内某个模型的独立用量查询。所述额度包括 5 小时、周与订阅月，不能用 Token Plan 个人版的月度 Credits 规则替代。

来源入口：[Coding Plan 常见问题](https://help.aliyun.com/zh/model-studio/coding-plan-faq)。

### 3.4 CLI 线索与授权边界

本地 `bailian-cli/reference/usage.md` 列出 `bl usage token-plan` 与 `bl usage coding-plan`，两者鉴权字段为 Console。官方用量文档的搜索结果也列出这两个命令。

这只能证明有文档化的 CLI 查询路径，不能证明它们可仅凭订阅 Key 使用，也不能证明所有账户均可独立导入、长期续期或在不安装 CLI 的情况下直接接入。

来源入口：[百炼 CLI 用量与配额](https://help.aliyun.com/zh/model-studio/cli/usage-quota)。

## 4. Codex 的待核实来源

本轮以下内容来自公开搜索结果，未取得完整源码，不据此冻结协议：

- [账户请求处理源码](https://github.com/openai/codex/blob/main/codex-rs/app-server/src/request_processors/account_processor.rs)
- [响应 schema](https://github.com/openai/codex/blob/main/codex-rs/app-server-protocol/schema/json/v2/GetAccountRateLimitsResponse.json)
- [额度查询测试](https://github.com/openai/codex/blob/main/codex-rs/app-server/tests/suite/v2/rate_limits.rs)

下一步需要核实：方法与字段版本、窗口可选性与真实持续时间、比例精度、账号及套餐范围、独立授权与续期、依赖组件许可及分发方式。不能把主次窗口硬编码为特定周期，也不能直接把本地 app-server 方法写成公开网络接口。

### 4.2 VERIFY-001 进展（2026-10-21）

已通过本机 `codex app-server generate-json-schema`（CLI 0.163.0）导出并核实协议：`initialize` → `initialized` → `account/rateLimits/read` 的 JSON-RPC 方法与行分隔帧格式；响应含 `usedPercent`（必选，语义为**已用**）、可选 `windowDurationMins`、可选 `resetsAt`（Unix 秒）、`planType` 枚举（含 `plus`）及 `rateLimitsByLimitId` 多窗口视图。剩余比例由应用侧 `max(0, 100 - usedPercent)` 派生，低额度判断使用该派生值。

据此实现了 Codex 垂直链路（Rust 适配器 + 统一状态层与后台刷新调度 + 前端额度卡），并用假 app-server 脚本完成进程、协议与解析的端到端测试（合成数据，不含真实账户）。前端通过 `get_app_state` 读取缓存状态、`refresh_usage` 触发刷新，并订阅 `app-state-changed`；查询失败保留上次成功快照。

**未在本会话执行真实账户读取**：真实额度返回会携带用户身份与用量数据，属于需用户本人查看的隐私内容。剩余/已用口径的最终确认，由用户运行应用后与 Codex 客户端显示对照；文档不记录具体额度数值。若应用显示的剩余比例与客户端相反，说明该字段语义与 schema 命名不符，需回到适配层修正换算。

**2026-10-22 本机运行补充**：用户本机运行的应用已完成一次真实 Codex 查询并写入状态快照；核对状态文件元数据可见 `authState=ok`、`planTier=plus`、`checkedAtEpoch` 有效。自动化会话未读取或记录任何额度数值；剩余/已用口径仍需用户本人对照客户端确认。

当前接入方式为“官方本地组件的受控只读使用”，与已确认原则的边界关系（正式版是否内置/依赖该 CLI）仍是 DEV-003 待决项；不影响本机走通链路。

### 4.1 用户界面参考补充

用户提供的 Codex 用量界面确认当前套餐为 Plus。界面标题为剩余用量，可见两个不同周期及各自的百分比、时间或日期表达。

这是人工展示参考，确认了剩余与已用比例不能混淆，并不能证明任一查询接口可用、窗口在所有套餐中固定，或截图中的日期已包含完整时区和时间戳。后续将真实查询结果与用户选择的界面核对，相关读取或查询仍需单独授权。

本仓库不保存原始截图、账户名称、截图源路径或真实剩余比例。所有后续测试样例使用明确的合成值或经授权脱敏的数据。

## 5. 对设计与计划的影响

- 数据模型应分别表达平台、套餐、额度单位、计量窗口、已用或剩余比例、可选绝对值与可选重置时间；不要求每个平台返回 token 数。
- 缺失字段必须是未知或不支持，不能默认补零；比例推导应保留来源、口径与精度。
- 多账户、独立授权及后台续期是能力验证的核心，而不是 UI 完成后再解决的细节。
- 查询授权与推理授权分开设计。不能为验证额度功能调用计费模型，也不要求用户把主账号高权限凭证交给应用。
- 平台规则及版本可能变化，适配层需要保留可识别的能力与失败信息，不让 UI 固定假设所有套餐都具有相同窗口。
- 百炼两类套餐已于 2026-10-23 完成真实读取验证并接入（见第 7 节）；Codex 适配器此前已完成。剩余待验证项主要是跨账号隔离、令牌续期与限流。

## 6. 下一步验证清单

1. 已完成：用户确认实际使用 Token Plan 个人版、Coding Plan 中国站。优先按这两类验证；不自动承诺团队版、国际站或其他存量套餐兼容。
2. 取得可访问的官方原文或公开源码，核实候选方法、许可及适用范围；未能访问的项目保留为待验证。
3. 给三类套餐分别列出允许的查询授权、最小权限、独立账户隔离及续期方式。
4. 在单独明确操作授权后，使用用户选择的测试账户验证；不扫描或改写现有工具配置，不保存未脱敏样例。
5. 对照平台真实显示，验证额度、周期、时间、查询错误与部分数据缺失，再确定字段级 IPC 与持久化设计。

## 7. 百炼接入验证（2026-10-23）

本节的真实读取由用户本人环境中的控制台会话完成；文档只记录协议与字段语义，不保存令牌、账号、UID 或具体额度数值。合成样例用于单元测试。

### 7.1 鉴权与查询路径

- 两类订阅额度都属于控制台（Console）数据，推理 API Key 不能替代。控制台令牌通过请求头 `Authorization: Bearer <access_token>` 使用。
- 网关：`https://bailian-cs.console.aliyun.com/cli/api.json?action=BroadScopeAspnGateway&product=sfm_bailian&api=<API 名>`。
- 请求体为 `application/x-www-form-urlencoded`：`params=<JSON>&region=cn-beijing`。`params` 包含 `Api`、`V: "1.0"` 与 `Data`；`Data.cornerstoneParam` 需包含 `protocol: "V2"`、`console: "ONE_CONSOLE"`、`productCode: "p_efm"`、`switchUserType: 3`、`consoleSite: "BAILIAN_ALIYUN"`。
- Token Plan 用量：API 名 `zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/usage`，请求数据为空对象。
- Token Plan 订阅：API 名 `zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/subscription`，请求 `queryInstanceInfoRequest = { commodityCode: "sfm_tokenplansolo_public_cn" }`，返回 `specCode`（lite / essential / standard / pro）、`status`、`remainingDays` 与 `endTime`。
- 控制台登录信息：API 名 `zeldaEasy.cornerstone-portal.cs-console.loginInfo`，返回 `aliyun.uid` 与 `aliyun.nickName`，可作为账号提示。
- Coding Plan：API 名 `zeldaEasy.broadscope-bailian.codingPlan.queryCodingPlanInstanceInfoV2`，请求 `queryCodingPlanInstanceInfoRequest = { commodityCode: "sfm_codingplan_public_cn", onlyLatestOne: true }`。
- 会话失效时，网关可能返回 HTTP 200 且在 `data.errorCode` 中包含 `NotLogined`；适配层据此归类为需要重新授权，而不是普通网络错误。

### 7.2 登录回调

官方控制台登录页支持 CLI 使用的回环回调：

1. 应用在 `127.0.0.1` 随机端口监听，生成随机 `state`。
2. 打开 `https://bailian.console.aliyun.com/console-login?notice=127.0.0.1:<port>?state=<state>`。
3. 登录成功后，控制台页面把 `access_token` 回传到该地址；应用校验 `state` 后只保存令牌。

应用没有修改 `bl` 配置，也不读取本机 `~/.bailian/config.json`。多账户场景需要在浏览器中切换对应的阿里云账号后重新授权。

### 7.3 字段语义

| 产品 | 返回字段 | 应用口径 | 能力缺口 |
| --- | --- | --- | --- |
| Token Plan 个人版 | `per1MonthPercentage`（0..1）、`per1MonthResetTime`（Unix 毫秒）；订阅接口的 `specCode`、`status`、`remainingDays` | 已用百分比四舍五入，剩余比例由应用派生；`specCode` 作为档位（Lite / Essential / Standard / Pro）展示；订阅月重置 | 用量接口不返回绝对 Credits 余额；订阅月按购买日滚动，不提供固定时长；无有效订阅时订阅接口可能缺少 `specCode` |
| Coding Plan 中国站 | `codingPlanInstanceInfos[]`，取 `status = VALID`；`codingPlanQuotaInfo.per5Hour / perWeek / perBillMonth` 各自的 `UsedQuota`、`TotalQuota`、`QuotaNextRefreshTime`；`instanceType` | 保留原始调用次数，分别展示 5 小时、1 周、订阅月三个窗口；比例由已用/总量派生 | 只提供调用次数，不提供 token 消耗或单模型用量；无有效订阅时返回空列表，展示为数据缺口而非错误 |
| 两者 | `cs-console.loginInfo` 的 `aliyun.nickName` / `aliyun.uid`；Coding Plan 响应中的 `userId` | `nickName` 不是纯数字时直接作为账号提示；否则用 `UID ***末四位` | 控制台网关拿不到阿里云登录名（`ACCOUNT_NAME` / `login_aliyunid`），该值只存在于浏览器页面配置或 Cookie；`nickName` 可能退化为数字 UID |

`per1MonthPercentage` 与 Coding Plan 的百分比都按“已用”解释；应用展示剩余比例时使用 `100 - used`，并在界面中明确标注“已用”。

### 7.4 仍未验证

- 第二个阿里云账号的独立授权与数据隔离，需要用户实际切换账号验证。
- 控制台令牌的有效期、续期方式以及过期后的页面行为；当前按“需要重新授权”处理。
- 限流阈值与合适的失败退避参数。
- Token Plan 与 Coding Plan 同时存在时的账号关系已按“同一账户两个产品”实现；其他套餐组合未验证。
- 未订阅 Coding Plan 时接口返回空列表，已按数据缺口处理；未订阅 Token Plan 时该接口的返回尚未验证，不能据此断言为零额度。
- 阿里云登录名（如控制台右上角展示的 `aliyunId`）来自浏览器页面配置或 Cookie，控制台令牌接口不返回该值。应用只在 `nickName` 是真实昵称时展示它，否则回退为脱敏 UID；如需显示完整登录名，需要另行确认是否允许浏览器 Cookie 导入或手动填写。

## 8. OpenCode Go、Command Code 与 Antigravity（2026-10-23）

本轮新增三个平台，优先级与边界如下。三者均未使用真实账户完成本机端到端验证，协议字段来自官方文档、官方 CLI 行为或公开实现，接入后仍需用户实际账户复核。

### 8.1 OpenCode Go

- 查询：`GET https://opencode.ai/zen/go/v1/usage`，使用 Go API Key 作为 Bearer 令牌。
- 返回：`usage.rolling` / `weekly` / `monthly`，每项包含 `status`、`percent`（已用百分比）与 `resetsAt`（RFC 3339）。`status = rate-limited` 时按 100% 处理。
- 口径：API 返回账户维度的汇总百分比，不按模型拆分；`rolling` 为 5 小时窗口，`weekly` 为 1 周窗口，`monthly` 按订阅周期滚动，应用不写死 30 天。
- 错误：401 视为 Key 失效；403 视为未开通 Go 订阅；429 与 5xx 分别提示稍后重试。
- 未验证：Go Plus 与 Go 的响应是否完全一致；订阅月重置时间是否始终返回；多工作区 Key 的口径差异。

### 8.2 Command Code

- 查询：使用 Provider API Key（`user_…`）访问 `GET https://api.commandcode.ai/alpha/billing/credits`，并可选调用 `/alpha/usage/summary` 与 `/alpha/billing/subscriptions`。
- 字段：`credits.monthlyCredits`（剩余）、`purchasedCredits`、`windowLimits.fiveHour` / `weekly` 的 `used`、`cap`、`resetAt`；`summary.totalMonthlyCredits` 为本账期已用；订阅接口提供 `planId` 与 `currentPeriodEnd`。
- 口径：月度上限由“已用 + 剩余”推导，百分比随已用/上限变化；滚动窗口直接使用已用/上限。辅助接口失败时保留 credits 窗口并记录能力缺口，不伪造月度上限。
- 错误：401/403 视为 Key 失效；404 提示接口不可用；429 与 5xx 分别提示稍后重试。
- 未验证：真实订阅账户的 `/alpha` 响应、Provider 计划（无窗口）与订阅计划之间的字段差异、月度账期与 `currentPeriodEnd` 的时区语义。

### 8.3 Google Antigravity

- 查询：调用官方 CLI 打印模式 `agy -p /usage --output-format json`，读取 `command.data.groups[].buckets[]` 的 `window`、`remaining_fraction` 与 `reset_time`。
- 版本门槛：仅 `agy` ≥ 1.1.11 支持；旧版会把 `/usage` 当成普通提示词并消耗额度，因此应用先执行 `agy --version`，不满足则拒绝查询。
- 过程控制：设置 `AGY_CLI_DISABLE_AUTO_UPDATE=true`，标准输入置空；查询带 60 秒超时，未登录等待交互时会被终止并归类为需要登录。
- 代理：设置页提供跟随系统 / 不走代理 / 指定代理三种模式，作用于全部 HTTP 查询与外部 CLI。跟随系统读取 Windows `ProxyEnable` / `ProxyServer` / `ProxyOverride` 或 macOS `scutil --proxy`，环境变量优先；`ProxyOverride` 转为 `NO_PROXY`。指定代理需为不含凭据的 HTTP/HTTPS 地址；PAC/自动配置脚本不解析。
- 边界：不读取 `agy` 的钥匙串、`~/.gemini/...` 令牌文件、IDE SQLite 或浏览器会话；账户来自用户本机 CLI 登录态。移除应用内账户不会退出 `agy`。
- 未验证：用户本机是否安装 `agy`、CLI 输出是否在所有版本保持蛇形字段、免费版与 Google AI 订阅的窗口差异。若后续版本改变字段，应在 `antigravity.rs` 的解析测试中更新合成样例。
