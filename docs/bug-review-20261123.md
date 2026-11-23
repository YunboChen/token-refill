# Bug 审查清单（2026-11-23）

- 分支：`fix/bug-review-20261123`（基于 `main` @ `b6a1749`）
- 方式：静态阅读源码，未编译、未运行测试
- 范围：`src/`（lib、panels）与 `src-tauri/src/`（state、relay、quota、各平台接入、update、lib）

## 汇总

| # | 级别 | 位置 | 问题 |
|---|------|------|------|
| 1 | 严重 | `src-tauri/src/state.rs:2369-2373` | 状态文件读取失败时以可写空状态启动，随后被覆盖，账户全丢 |
| 2 | 高 | `state.rs:805` + `src/lib/attention.ts` | 剩余 0.x% 向下取整为 0，被误判为「额度已用尽」 |
| 3 | 中 | `state.rs:801-814` | 总额 ≤ 0 时返回 0%（红色），应为未知 |
| 4 | 中 | `src-tauri/src/relay.rs:524-551` | 编辑中转站：读-联网-写之间无校验，可能覆盖并发刷新 |
| 5 | 中 | `antigravity.rs` / `opencode.rs` / `codex.rs` | Windows 下超时只杀 `.cmd` 外壳，留下孤儿 `node.exe` |
| 6 | 中低 | `antigravity.rs:195,242`、`opencode.rs:221` | 输出达上限后停止读取，子进程管道写满卡到超时 |
| 7 | 待确认 | `commandcode.rs:126-129` | `totalMonthlyCredits` 被当作已用量，可能实为总额 |
| 8 | 低 | 多处 | 若干小问题，见下文 |

## 详情

### 1. 状态文件读取失败导致数据被覆盖（严重）

```rust
let raw = match fs::read_to_string(path) {
    Ok(raw) => raw,
    Err(_) => return LoadedState::writable(default_state()),
};
```

任何读取错误（被杀毒/同步盘占用、权限、非 UTF-8）都会得到**可写**的空状态，下一次 `update_soft` 即调用 `write_state` 覆盖原 `state.json`。注释「后续成功写入前不会覆盖原文件」与实现不符。

**建议**：仅 `ErrorKind::NotFound` 返回可写默认状态；其他错误先备份原文件，或以只读模式启动并提示用户。

### 2. 剩余比例向下取整导致误报「已用尽」

`derived_remaining_percent` 使用 `floor`，剩余 0 < x < 1% 时得到 0；`attention.ts` 的 `exhaustedDetail` 以 `remainingPercent === 0` 判定用尽，账户被放入「需处理」并显示「额度已用尽」。

**建议**：剩余量 > 0 时百分比至少为 1；或判定用尽时使用 `remainingAmount === 0`。

### 3. 总额缺失时显示 0%

`derived_remaining_percent` / `derived_used_percent` 在 `total <= 0` 时返回 `0`，前端按危险色显示。前端 `relayPercent` 同类情况返回 `null`（未提供），两端不一致。

**建议**：返回 `Option<i64>`，总额无效时为 `None`。

### 4. 中转站编辑的竞态

`relay.rs` 编辑流程：加锁读取条目 → 释放锁联网检测 → 重新加锁写回。期间若该条目被自动刷新或其他操作修改，会被旧值覆盖；`account_token` 也使用了旧值。

**建议**：写回前比较修订号或校验 `base_url`/`api_key` 未变化，冲突时重新读取或报错。

### 5. Windows 下孤儿进程

`antigravity.rs`（261/269/276）、`opencode.rs`（255/262）、`codex.rs`（907）超时或出错时调用 `child.kill()`。CLI 经 `.cmd` 启动时只会结束 `cmd.exe`，真正的 `node.exe` 继续运行，自动刷新下会累积。

`cursor.rs:561` 的 `kill_child` 已用 `taskkill /T /F` 正确处理。

**建议**：把 `kill_child` 提到 `cli.rs` 作为公共函数，各处统一使用。

### 6. 输出达上限后子进程挂起

`read_capped` 使用 `take(MAX_OUTPUT_BYTES).read_to_end` 后不再读取；antigravity 的 stderr 线程达到上限即 `break`。子进程继续写输出时管道写满而阻塞，直到超时。

`cursor.rs:440` 的做法正确：超过上限后继续读取并丢弃。

**建议**：达到上限后继续 drain 剩余输出（丢弃），直至 EOF。

### 7. Command Code 月度用量字段（待确认）

```rust
let used = summary.and_then(|v| number(v.get("totalMonthlyCredits")))...;
let total = used.map(|used| used + remaining);
```

若 `totalMonthlyCredits` 是月度总额而非已用量，总额被重复累加、百分比错误。需对照真实接口响应确认。

### 8. 小问题

- `src/lib/format.ts` `normalizeWindowName`：子串匹配过宽，`Monday`/`today` 会被识别为「1 天」，`15h` 识别为「5 小时」。
- `format.ts` `durationLabel`：取整过粗，90 分钟显示「2 小时」，2000 分钟显示「1 天」。
- `src/lib/relayFormat.ts` `relayHost`：注释称只显示主机名，实际只去掉协议，端口和路径仍保留。
- `bailian.rs` / `workbuddy.rs` `random_hex`：`getrandom` 失败时回退到时间戳，生成的 nonce/state 可预测；建议失败时直接报错。
- `antigravity.rs:245`：每读一块就从头扫描整个缓冲区找认证标记（O(n²)），并可能重复发送信号。
- `src/panels/RelayPanel.tsx:81`：用 `Number()` 解析提醒金额，`0x10`、`1e2` 可通过校验；建议用 `^\d+(\.\d{1,2})?$`。
- `src/panels/OverviewPanel.tsx:980`：`nowEpoch` 每 60 秒更新，重置后「需处理」最多延迟 1 分钟消失。
- `src/panels/SettingsPanel.tsx:136`：`refreshFill` 实为低额度阈值滑块填充，命名有误。

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
