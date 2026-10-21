# 平台接入可行性记录

- 记录日期：2026-10-21
- 阶段：资料初查，不是接入验证通过
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

据此实现了 Codex 垂直链路（Rust 命令 `refresh_codex_usage` + 前端额度卡），并用假 app-server 脚本完成进程、协议与解析的端到端测试（合成数据，不含真实账户）。

**未在本会话执行真实账户读取**：真实额度返回会携带用户身份与用量数据，属于需用户本人查看的隐私内容。剩余/已用口径的最终确认，由用户运行应用后与 Codex 客户端显示对照：当前截图（5 小时剩 96%、1 周剩 87%）应对应 `usedPercent` 约 4 与 13。若数值相反，说明该字段语义与 schema 命名不符，需回到适配层修正换算。

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
- 原计划 PLAN-003 仍未通过；目前仅资料初查，三类套餐都没有完成真实接入验证。

## 6. 下一步验证清单

1. 已完成：用户确认实际使用 Token Plan 个人版、Coding Plan 中国站。优先按这两类验证；不自动承诺团队版、国际站或其他存量套餐兼容。
2. 取得可访问的官方原文或公开源码，核实候选方法、许可及适用范围；未能访问的项目保留为待验证。
3. 给三类套餐分别列出允许的查询授权、最小权限、独立账户隔离及续期方式。
4. 在单独明确操作授权后，使用用户选择的测试账户验证；不扫描或改写现有工具配置，不保存未脱敏样例。
5. 对照平台真实显示，验证额度、周期、时间、查询错误与部分数据缺失，再确定字段级 IPC 与持久化设计。
