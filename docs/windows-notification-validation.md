# Windows 授权失效提醒验收清单

本文件用于在 Windows 10/11 x64 上验收「授权失效提醒可靠性」这一改动。对应的设计说明见
[软件设计](./software-design.md) 的「提醒判断与去重」一节；macOS 侧目前只有单元测试覆盖，
真实通知与权限切换验收仍待补（见[开发计划](./development-plan.md) 的 DEV-009）。Windows
兼容层背景见 [Windows 兼容设计](./windows-support.md)。

## 1. 改动摘要（需要验证的行为）

- 各订阅平台（Codex、百炼、OpenCode Go、Command Code、Antigravity）的授权失效状态统一生成待提醒记录，不再由各平台分别判断。
- 提醒记录包含 `noticeId`、`delivered`、`attempts`、`nextAttemptEpoch`；`delivered` 只在系统确认接收后写入，不再用 `authState` 充当发送成功回执。
- 发送失败（权限拒绝、未允许、发送报错）时按 60、120、240、480、960、1920、3600 秒退避重试，之后封顶每小时一次；重试不重新查询额度，也不重复申请权限。
- 提醒开关关闭时暂停发送，重新开启后由调度器继续尝试；成功后关闭再开启不会重发。
- 授权恢复、重新授权、移除账户时清理记录；旧回调不能确认新一轮失效。
- 状态文件新增 `reminders.authExpired` 字段，带默认值，`schemaVersion` 保持 1：旧版本可继续读写同一文件（会丢弃该字段），降级不会丢账户。
- 用户可见变化：权限允许后，失败的授权提醒会在下次调度时补发；已成功的提醒在刷新、重启与开关切换后不重复。

不在本次范围：中转站（New API / Sub2API）的失效提醒（尚未接入）；低额度与重置提醒只做回归，不重新设计。

## 2. Windows 与 macOS 的差异（先说清限制）

| 项目 | macOS | Windows |
| --- | --- | --- |
| 发送实现 | `UNUserNotificationCenter` | `tauri-plugin-notification`（`notify-rust`） |
| 权限流程 | 启动时申请一次，可区分未决定 / 拒绝 / 允许 | 没有应用内授权申请；由「设置 → 系统 → 通知」控制 |
| 发送结果 | 有完成回调，可区分系统拒绝、超时与未知 | 只有调用是否报错；**没有投递回执** |
| 稳定通知标识 | `send_with_identifier` 使用 `noticeId` | 插件不支持标识，退化为普通发送 |

由此产生两个需要在实机确认的问题：

1. 系统通知被关闭时，插件是否仍返回成功？如果仍返回成功，`delivered` 会被错误写入，本次改动在 Windows 上不会重试。这属于平台能力限制，需要在结果记录中如实标注，不能写成“已验证”。
2. 免安装 exe 与 NSIS 安装版的 Toast 注册方式不同（快捷方式 / AppUserModelID）。两种形态都要单独确认通知能出现。

## 3. 环境与构建

- Windows 10/11 x64，已安装 WebView2 Runtime（Win11 自带；Win10 通常随 Edge 安装）。
- Rust 使用 MSVC 目标：`rustup default stable-x86_64-pc-windows-msvc`。
- Node.js ≥ 22.12，pnpm 12；分支 `fix/auth-expired-notification-delivery`。

使用**测试标识**构建，数据目录为 `%APPDATA%\dev.tokenrefill.desktop.test\`，与正式版隔离：

```powershell
pnpm install --frozen-lockfile
pnpm build

# 免安装 exe
pnpm tauri build --no-bundle --config src-tauri/tauri.test.conf.json
# 产物：src-tauri\target\release\token-refill.exe

# NSIS 安装版
pnpm tauri build --bundles nsis --config src-tauri/tauri.test.conf.json
# 产物：src-tauri\target\release\bundle\nsis\*-setup.exe
```

> 验证前先退出已安装的 Token Refill（正式版与测试版均可共存，但同一标识只允许一个实例）。
> 不要用 `pnpm tauri dev` 做通知验收：它额外启动 Vite，且窗口生命周期与发布包不同。

## 4. 隔离测试数据（合成账户，不使用真实授权）

验收不需要真实账户。先退出应用，然后写入一份 v1 合成状态：

```powershell
$dir = Join-Path $env:APPDATA 'dev.tokenrefill.desktop.test'
New-Item -ItemType Directory -Force -Path $dir | Out-Null

$state = @'
{
  "schemaVersion": 1,
  "revision": 0,
  "accounts": [{
    "accountId": "bailian-synthetic-notification",
    "platform": "bailian",
    "displayName": "合成通知验证账户（无真实授权）",
    "authState": "needs-reauth",
    "source": "managed",
    "identityHint": null,
    "products": [
      {"productId": "bailian-token-plan", "kind": "bailian-token-plan", "displayName": "Token Plan", "planTier": null, "active": null, "snapshot": null, "query": {"refreshing": false, "lastAttemptEpoch": null, "lastError": null}},
      {"productId": "bailian-coding-plan", "kind": "bailian-coding-plan", "displayName": "Coding Plan", "planTier": null, "active": null, "snapshot": null, "query": {"refreshing": false, "lastAttemptEpoch": null, "lastError": null}}
    ]
  }],
  "preferences": {"refreshMinutes": 60, "lowQuotaEnabled": false, "lowQuotaThreshold": 20, "authExpiredEnabled": true, "resetNoticeEnabled": false, "autoLaunchEnabled": false, "proxyMode": "none", "proxyUrl": null},
  "reminders": {"lowQuota": {}}
}
'@

Set-Content -Path (Join-Path $dir 'state.json') -Value $state -Encoding UTF8
```

合成账户没有 `bailian-profiles\bailian-synthetic-notification\`，启动后的第一次查询会返回
`auth-expired`（「未找到百炼授权，请重新授权。」），正好触发本次改动的提醒链路。

启动时禁用真实 CLI 与密钥回退，避免访问任何真实账户：

```powershell
$env:CODEX_BIN   = 'C:\nonexistent\codex.exe'
$env:OPENCODE_BIN = 'C:\nonexistent\opencode.exe'
$env:AGY_BIN     = 'C:\nonexistent\agy.exe'
Remove-Item Env:OPENCODE_GO_API_KEY -ErrorAction SilentlyContinue
```

## 5. 日志与状态观察口径

### 5.1 带日志启动

GUI 子系统程序从终端启动时仍继承 stdio：

```powershell
$exe = "C:\path\to\token-refill.exe"
$log = Join-Path $PWD "tr-notify.log"
Remove-Item $log -ErrorAction SilentlyContinue
$p = Start-Process -FilePath $exe -RedirectStandardError $log -PassThru
"PID=$($p.Id)"
Get-Content $log -Wait
```

日志关键字：

| 日志 | 含义 |
| --- | --- |
| `发送「授权失效提醒」失败，将按退避重试：...` | 本次发送失败，已留在待发送队列 |
| `预留授权失效提醒失败：...` | 状态落盘失败，本次跳过（不应持续出现） |
| `保存应用状态失败：...` | 状态写入失败，需要记录 |

成功的发送不会写日志，以 `state.json` 的 `delivered` 为准。

### 5.2 读取提醒记录

```powershell
$statePath = Join-Path $env:APPDATA 'dev.tokenrefill.desktop.test\state.json'
$s = Get-Content $statePath -Raw | ConvertFrom-Json
"schemaVersion = $($s.schemaVersion)"
$m = $s.reminders.authExpired.'bailian-synthetic-notification'
$m | Format-List
if ($m.nextAttemptEpoch) {
    [DateTimeOffset]::FromUnixTimeSeconds([int64]$m.nextAttemptEpoch).LocalDateTime
}
```

判读规则：

- `delivered = true` 且 `nextAttemptEpoch = null`：系统已接收，不再重试。
- `delivered = false` 且 `nextAttemptEpoch ≈ now + 退避秒数`：发送失败或尚未到期。
- `attempts` 只增不减；重启后不清零（除非账户恢复或提醒记录被清理）。

退避间隔（`nextAttemptEpoch - 尝试时刻`）：60、120、240、480、960、1920、3600 秒，之后保持 3600 秒。
验收时不需要真的等满上界，按文件里的数值核对公式即可。

## 6. 验证项

逐项填写第 8 节的结果表。建议按 A → B → C → D 顺序执行。

### A. 首次失效与迁移

**N-01 旧状态补建提醒记录**
1. 按第 4 节写入不含 `authExpired` 的合成状态（`schemaVersion` 保持 1），按第 3 节启动。
2. 等 15 秒，运行 5.2 的读取脚本。
3. 预期：`schemaVersion = 1`；合成账户仍在，`authState` 为 `needs-reauth`；`reminders.authExpired` 出现该账户的 `noticeId`。
4. 预期：目录中没有 `.corrupt-*` 备份文件（备份只应在文件损坏或版本未知时出现）。

**N-02 首次失效进入待发送**
1. 继续观察 5.1 日志与 5.2 状态。
2. 预期：`delivered = false`，`attempts ≥ 1`，`nextAttemptEpoch` 为将来时间；若系统通知已允许，可能已直接成功（见 N-03）。
3. 预期：日志中没有额度接口报错以外的异常；界面总览显示合成账户「需要重新授权」。

### B. 成功路径与去重（核心）

**N-03 允许通知后成功发送**
1. 确认「设置 → 系统 → 通知 → Token Refill Test」已开启（以及「通知中心」）。
2. 等待一次重试到期（最长 1 小时；也可关闭再开启应用内「授权失效提醒」触发下一次调度）。
3. 预期：`delivered = true`，`nextAttemptEpoch = null`；Windows 通知中心出现一条「授权失效提醒」。
4. 记录：横幅是否出现、是否只出现一次（专注助手等系统行为单独记录，不作为代码失败）。

**N-04 成功后重启不重复**
1. 退出应用，再启动（保留第 4 节以外的环境变量）。
2. 等 2 分钟，期间可手动点一次刷新。
3. 预期：`attempts` 不变，`delivered` 仍为 `true`，没有新的通知。

**N-05 开关切换不重发**
1. 设置页关闭「授权失效提醒」，等 2 分钟。
2. 再开启，等 2 分钟。
3. 预期：`attempts` 不变，没有新的通知；开关状态正确写入 `preferences.authExpiredEnabled`。

**N-06 关闭时暂停、开启后补发（未成功的提醒）**
1. 重新写入第 4 节的 v1 状态（形成一条新的待发送记录）。
2. 关闭应用内「授权失效提醒」开关，等 2 分钟。
3. 预期：`attempts` 不增加，`delivered = false`。
4. 开启应用内开关，等待到期。
5. 预期：按当前系统通知状态继续尝试；成功则 `delivered = true`，失败则 `attempts` 递增。

### C. 失败、退避与平台限制

**N-07 系统通知关闭时的行为（关键平台结论）**
1. 在「设置 → 系统 → 通知」中关闭 Token Refill Test 的通知。
2. 写入一条新的待发送记录并启动，等 3 分钟。
3. 分别记录：日志是否出现「发送…失败」；`delivered` 是否被写成 `true`。
4. 判读：
   - 若 `delivered = false` 且 `attempts` 递增、间隔符合 60/120/240 秒：说明插件能感知失败，重试生效。
   - 若 `delivered = true` 但通知中心没有记录：说明插件在通知关闭时仍返回成功，Windows 上缺少投递回执。
     此时本次改动**不会**自动重试，需在第 8 节标注为平台限制，并同步到
     [Windows 兼容设计](./windows-support.md) 的已知限制，不能写成“Windows 已支持失败重试”。
5. 恢复系统通知开关。

**N-08 退避公式与上界**
1. 在 N-07 的失败状态下连续观察 3–4 次重试（若失败不可复现，可跳到第 3 步只核对公式）。
2. 预期：`attempts` 逐次递增，`nextAttemptEpoch` 相对本次尝试依次为 60、120、240、480 秒。
3. 用文件中的数值人工核对是否满足 `min(60 × 2^(attempts-1), 3600)`；不必等待封顶。
4. 预期：失败期间没有因为刷新而立即重发（两次尝试的最小间隔不小于上表）。

**N-09 重启不绕过退避**
1. 在 `nextAttemptEpoch` 尚未到期时退出并重启应用。
2. 2 分钟内观察 `attempts` 与 `nextAttemptEpoch`。
3. 预期：`attempts` 不增加，`nextAttemptEpoch` 保持原值（可能被新一轮上界改写，但不会变成“立即重试”）。

**N-10 非授权错误不提醒**
1. 用一把无效的合成 API Key 添加 Command Code 或 OpenCode Go 账户，或断开网络后等一次刷新。
2. 预期：查询失败但账户不出现新的授权失效记录；只有明确的 401/403 类错误才进入提醒。

### D. 平台形态与回归

**N-11 免安装 exe 的 Toast**
1. 用 `--no-bundle` 产物重复 N-03。
2. 预期：通知能出现。若不能出现但安装版可以，记录为便携版的 AppUserModelID/快捷方式限制。

**N-12 NSIS 安装版的 Toast**
1. 安装 NSIS 测试版，从开始菜单与安装目录分别启动，重复 N-03。
2. 预期：通知能出现，且与便携版不互相覆盖数据目录（同为测试标识，数据共享是预期行为）。

**N-13 前后台与最小化**
1. 分别在主窗口前台、最小化、关闭到托盘三种状态下触发一次提醒。
2. 预期：三种状态都不丢失提醒；是否弹横幅由系统与专注模式决定，单独记录。

**N-14 数据与凭证检查**
1. 用文本编辑器打开 `%APPDATA%\dev.tokenrefill.desktop.test\state.json`。
2. 预期：只有账户名、平台、快照与提醒记录；没有令牌、Cookie、API Key 或完整邮箱。
3. 检查 `tr-notify.log`：只有失败原因与退避提示，不含凭证。

**N-15 低额度与重置提醒回归**
1. 打开低额度提醒（阈值 50%），用合成快照或真实账户触发一次。
2. 预期：低额度提醒不重复，重置提醒仍默认关闭；本次改动没有破坏这两条链路。

### 通过标准

- N-01 至 N-06、N-09、N-10、N-14、N-15 必须通过。
- N-07 必须给出明确结论（可重试 / 平台无回执）；若是后者，按第 2 节与 N-07 第 4 步记录为已知限制。
- N-11、N-12 至少完成一种；另一种若环境不具备则标记「跳过」并说明原因。
- N-08 至少核对我一次实际退避数值；其余按公式人工核对。
- 任一场景出现以下情况即判失败：成功后重启又发一次；关闭提醒仍有新提醒；状态文件丢失账户或快照；日志泄露凭证。

## 7. 清理

```powershell
# 退出应用后删除测试数据（只会删测试标识目录）
Remove-Item -Recurse -Force (Join-Path $env:APPDATA 'dev.tokenrefill.desktop.test') -ErrorAction SilentlyContinue
```

正式版目录 `%APPDATA%\dev.tokenrefill.desktop\` 不应在本次验收中被修改。

## 8. 结果记录

| ID | 结果（通过/失败/跳过） | 观察值（attempts、delivered、间隔） | 备注 |
| --- | --- | --- | --- |
| N-01 旧状态补建提醒记录 |  |  |  |
| N-02 首次失效进入待发送 |  |  |  |
| N-03 允许通知后成功发送 |  |  |  |
| N-04 成功后重启不重复 |  |  |  |
| N-05 开关切换不重发 |  |  |  |
| N-06 关闭暂停、开启补发 |  |  |  |
| N-07 系统通知关闭时行为 |  |  |  |
| N-08 退避公式与上界 |  |  |  |
| N-09 重启不绕过退避 |  |  |  |
| N-10 非授权错误不提醒 |  |  |  |
| N-11 免安装 exe 的 Toast |  |  |  |
| N-12 NSIS 安装版 Toast |  |  |  |
| N-13 前后台与最小化 |  |  |  |
| N-14 数据与凭证检查 |  |  |  |
| N-15 低额度与重置回归 |  |  |  |

## 9. 失败时收集

- `tr-notify.log`（完整 stderr）。
- `state.json` 中 `reminders.authExpired` 段落（脱敏，可直接贴）。
- Windows 版本（`winver`）、WebView2 Runtime 版本：
  `Get-Item "$env:ProgramFiles(x86)\Microsoft\EdgeWebView\Application\*\msedgewebview2.exe" | Select-Object -Last 1 VersionInfo`
- 安装形态（便携 exe / NSIS）、系统通知开关状态、专注模式状态。
- 失败项复现步骤与截图（通知中心记录、设置页开关状态）。