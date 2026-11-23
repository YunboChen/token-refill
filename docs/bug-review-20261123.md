# Bug 审查清单（2026-11-23）

- 分支：`fix/bug-review-20261123`（基于 `main` @ `b6a1749`）
- 方式：静态阅读源码，未编译、未运行测试
- 范围：`src/`（lib、panels）与 `src-tauri/src/`（state、relay、quota、各平台接入、update、lib）
- 处理：同分支后续提交逐条修复，状态见下表

## 汇总

| # | 级别 | 位置 | 问题 | 处理 |
|---|------|------|------|------|
| 1 | 严重 | `src-tauri/src/state.rs:2369-2373` | 状态文件读取失败时以可写空状态启动，随后被覆盖，账户全丢 | 已修 |
| 2 | 高 | `state.rs:805` + `src/lib/attention.ts` | 剩余 0.x% 向下取整为 0，被误判为「额度已用尽」 | 已修 |
| 3 | 中 | `state.rs:801-814` | 总额 ≤ 0 时返回 0%（红色），应为未知 | 未修（不可触发，见下） |
| 4 | 中 | `src-tauri/src/relay.rs:524-551` | 编辑中转站：读-联网-写之间无校验，可能覆盖并发刷新 | 已修 |
| 5 | 中 | `antigravity.rs` / `opencode.rs` / `codex.rs` | Windows 下超时只杀 `.cmd` 外壳，留下孤儿 `node.exe` | 已修 |
| 6 | 中低 | `antigravity.rs:195,242`、`opencode.rs:221` | 输出达上限后停止读取，子进程管道写满卡到超时 | 已修 |
| 7 | 待确认 | `commandcode.rs:126-129` | `totalMonthlyCredits` 被当作已用量，可能实为总额 | 未修（需对照真实响应） |
| 8 | 低 | 多处 | 若干小问题，见下文 | 大部分已修 |

## 详情

### 1. 状态文件读取失败导致数据被覆盖（严重）— 已修

```rust
let raw = match fs::read_to_string(path) {
    Ok(raw) => raw,
    Err(_) => return LoadedState::writable(default_state()),
};
```

任何读取错误（被杀毒/同步盘占用、权限、非 UTF-8）都会得到**可写**的空状态，下一次 `update_soft` 即调用 `write_state` 覆盖原 `state.json`。注释「后续成功写入前不会覆盖原文件」与实现不符。

**处理**：仅 `ErrorKind::NotFound` 走可写默认状态；其他错误先复制备份（`state.json.unreadable-<ts>`），备份成功才继续可写，备份失败改为只读启动。两种结果都通过 `state_file_notice` 在界面提示，备份成功时给出备份路径与恢复方法。新增两个单元测试覆盖备份成功与只读两条路径。

### 2. 剩余比例向下取整导致误报「已用尽」— 已修

`derived_remaining_percent` 使用 `floor`，剩余 0 < x < 1% 时得到 0；`attention.ts` 的 `exhaustedDetail` 以 `remainingPercent === 0` 判定用尽，账户被放入「需处理」并显示「额度已用尽」。

**处理**：剩余量 > 0 时百分比至少为 1%；`used_percent` 由 `100 - remaining` 派生，会同步从 100 变成 99。新增单元测试。

### 3. 总额缺失时显示 0% — 未修

`derived_remaining_percent` / `derived_used_percent` 在 `total <= 0` 时返回 `0`，前端按危险色显示。前端 `relayPercent` 同类情况返回 `null`（未提供），两端不一致。

**未修原因**：唯一调用方 `workbuddy_group_window` 已先 `filter(|value| *value > 0.0)`，`total <= 0` 分支当前不可触发。若以后新增调用方，需要改为 `Option<i64>`。

### 4. 中转站编辑的竞态 — 已修

`relay.rs` 编辑流程：加锁读取条目 → 释放锁联网检测 → 重新加锁写回。期间若该条目被自动刷新或其他操作修改，会被旧值覆盖；`account_token` 也使用了旧值。

**处理**：写回前比较 `base_url`、`api_key`、`account_token`、`account_user_id`，任一变化则报错请用户重试。其余可并发修改的路径已核对：`set_label` 不写这些字段，`refresh_with` 对手动条目写回相同值，来源引用条目在读取阶段就拒绝编辑。新增一个用延迟回环服务构造竞态的测试，覆盖冲突分支。

### 5. Windows 下孤儿进程 — 已修

`antigravity.rs`（261/269/276）、`opencode.rs`（255/262）、`codex.rs`（907）超时或出错时调用 `child.kill()`。CLI 经 `.cmd` 启动时只会结束 `cmd.exe`，真正的 `node.exe` 继续运行，自动刷新下会累积。

**处理**：`cursor.rs` 的 `kill_child` 上提为 `cli::kill_process_tree`，各处统一使用。注意 Unix 分支仍只结束直接子进程（现有 CLI 均为原生进程或 shebang 直通，暂未改用进程组）。

### 6. 输出达上限后子进程挂起 — 已修

`read_capped` 使用 `take(MAX_OUTPUT_BYTES).read_to_end` 后不再读取；antigravity 的 stderr 线程达到上限即 `break`。子进程继续写输出时管道写满而阻塞，直到超时。

**处理**：新增 `cli::read_capped`，达到上限后继续读取并丢弃直到 EOF；antigravity 的 stderr 循环同样持续排空。认证标记检测改为只扫描新数据与 64 字节重叠区，且只通知一次。`read_capped` 补了「保留前缀并读到 EOF」与「读错误时返回已有输出」两个单元测试。

### 7. Command Code 月度用量字段（待确认）— 未修

```rust
let used = summary.and_then(|v| number(v.get("totalMonthlyCredits")))...;
let total = used.map(|used| used + remaining);
```

若 `totalMonthlyCredits` 是月度总额而非已用量，总额被重复累加、百分比错误。需对照真实接口响应确认；本轮未改动。

### 8. 小问题

- 已修 `src/lib/format.ts` `normalizeWindowName`：英文按起始词边界匹配，`Monday`/`today` 不再识别为「1 天」，`15h` 不再识别为「5 小时」；同时容忍 `week_limit`、`monthly_reset` 这类带下划线的名字。
- 已修 `format.ts` `durationLabel`：非整数时长保留一位小数，90 分钟显示「1.5 小时」。
- 已修 `src/lib/relayFormat.ts` `relayHost`：去掉末尾斜杠并更正注释（保留端口与路径）。
- 已处理 `bailian.rs` / `workbuddy.rs` `random_hex`：`getrandom` 失败时改用每块新的 `RandomState` 哈希，混入时间与计数，不再仅依赖时间戳（未改为直接报错）。
- 已修 `antigravity.rs:245`：每块都从头扫描整个缓冲区的问题（见第 6 条）。
- 已修 `src/panels/RelayPanel.tsx:81`：提醒金额只接受普通十进制写法，拒绝 `0x10`、`1e2`，并允许 `.5`。
- 未修 `src/panels/OverviewPanel.tsx:980`：`nowEpoch` 每 60 秒更新，重置后「需处理」最多延迟 1 分钟消失。
- 已修 `src/panels/SettingsPanel.tsx:136`：`refreshFill` 更名为 `thresholdFill`。

## 已检查未发现问题

- 状态/凭证写入：临时文件 + 原子替换，Unix 权限 600。
- Mutex 中毒恢复；刷新线程 `catch_unwind` 兜底，不会卡在「查询中」。
- 更高 schema 版本文件以只读运行，不降级覆盖。
- 中转站地址仅允许 https（回环除外），禁止账号信息与查询参数；`quota_per_unit ≤ 0` 回退默认值。
- OAuth 回调 HTTP 读取有长度上限与 state 校验。
- `open_external_url` 仅允许 https、过滤控制字符；Windows 用 `ShellExecuteW`，macOS 固定 `/usr/bin/open`。
- 前端事件订阅在卸载/StrictMode 下正确退订；状态按修订号丢弃旧快照；定时器与监听均清理。

## 未覆盖

- `floating_ball.rs`、`tray.rs` 中的窗口定位与动画逻辑（更适合运行时测试）。
- CI 工作流、构建配置。
