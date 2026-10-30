# Windows 内存优化验证清单

本文件用于在 Windows 10/11 x64 上验收「窗口 WebView 按需创建、按空闲销毁」这一改动。
对应的设计说明见 [软件设计](./software-design.md) 的「菜单栏与快速查看面板」一节，
macOS 侧已完成的验证记录见 [开发计划](./development-plan.md) 2026-10-28 条目。

## 1. 改动摘要（需要验证的行为）

- `tray-panel` 在 `tauri.conf.json` 中改为 `create: false`：启动时**不再**预创建菜单栏面板，第一次点开时才创建。
- 面板与主窗口隐藏满 **60 秒**后销毁 WebView（只销毁窗口，不结束进程），再次打开时重建。
- 重建窗口先隐藏创建，等页面加载完成（或 1.5 秒兜底）后再显示，避免白屏。
- 登录流程或更新下载进行期间，主窗口**不会**被空闲销毁。
- 销毁最后一个窗口不能导致应用退出（`RunEvent::ExitRequested` 已被拦截）。
- 用户可见变化：面板/主窗口在空闲销毁后重新打开会有一次短暂加载（预计 200–600 ms）；主窗口会重新居中、回到「总览」页签。

不在本次范围：Codex 查询子进程（`node` + `codex app-server`）的串行化与常驻复用。

## 2. 环境与构建

- Windows 10/11 x64，已安装 WebView2 Runtime（Win11 自带；Win10 通常随 Edge 安装）。
- Rust 使用 MSVC 目标：`rustup default stable-x86_64-pc-windows-msvc`。
- Node.js ≥ 22.12，pnpm 12；仓库分支 `feat/lazy-webview-lifecycle`。

```powershell
pnpm install --frozen-lockfile
pnpm build
pnpm tauri build --no-bundle
# 产物：src-tauri\target\release\token-refill.exe
```

> 用 release 产物验证内存和进程数；`pnpm tauri dev` 会额外启动 Vite，进程数不能作为基线。
> 验证前先退出已安装的 Token Refill，避免单实例插件把新进程直接结束。

### 日志

窗口与托盘生命周期日志同时写到 stderr 和文件：

```
%APPDATA%\dev.tokenrefill.desktop\logs\window.log      （正式版）
%APPDATA%\dev.tokenrefill.desktop.test\logs\window.log （测试版）
```

直接双击或从开始菜单启动也能拿到日志，不需要重定向。单文件超过 256 KiB 轮转为
`window.log.1`。想在终端里同时看实时输出时仍可重定向：

```powershell
$exe = "C:\path\to\token-refill.exe"
$p = Start-Process -FilePath $exe -RedirectStandardError (Join-Path $PWD "tr-verify.log") -PassThru
"PID=$($p.Id)"
Get-Content (Join-Path $PWD "tr-verify.log") -Wait
```

日志关键字：

| 日志 | 含义 |
| --- | --- |
| `[app] 启动 <名称> <版本>（<平台> / <渠道>）` | 进程启动，日志可用 |
| `[tray] 点击 Left/Down`、`Left/Up` | 托盘左键事件到达主线程；**没有这行说明点击没到事件循环**（主线程被阻塞或图标属于已退出的进程） |
| `[tray] 菜单：打开主窗口` / `查看额度` / `退出` | 右键菜单项被选中 |
| `[window] 创建 tray-panel（无 WebView 窗口可用）` | 面板按需创建开始 |
| `[window] tray-panel 加载耗时 N ms` | 面板从创建到页面加载完成 |
| `[window] 主窗口重建（无 WebView 窗口可用）` / `主窗口重建加载耗时 N ms` | 主窗口按空闲策略重建 |
| `[window] 显示主窗口 visible=true focused=…` | 主窗口显示结果；`focused=false` 说明前台激活被系统拒绝 |
| `[window] tray-panel 已显示` | 面板显示成功（有点击但没有这一行说明显示没生效） |
| `[window] tray-panel 再次点击收起` / `失焦收起` | 面板被图标点击 / 失焦收起（后者需要确实获得过焦点） |
| `[window] tray-panel 未能获得焦点，补一次` | 显示后没拿到激活态，触发了补聚焦 |
| `[window] tray-panel 加载中再次点击，取消本次显示` | 加载期间的第二次左键点击被当作取消 |
| `[window] tray-panel 仍不可见，重试显示` / `[window] 主窗口仍不可见，重试显示` | 兜底显示后窗口仍不可见，补了一次显示（**出现即异常**） |
| `[window] 销毁空闲窗口 tray-panel` / `main` | 隐藏满 60 秒，开始销毁 |
| `[window] ... 加载超时，直接显示` | 1.5 秒兜底触发（正常应很少出现） |
| `[window] 创建 tray-panel 失败：...` / `重建主窗口失败：...` | 创建失败，需要记录 |
| `[window] 窗口已全部销毁，拦截退出请求` | 空闲销毁后应用仍常驻（正常） |

## 3. 进程与内存口径

Windows 上 `msedgewebview2.exe` 是**所有** WebView2 应用共用的进程名，不能直接按名字求和。
只统计 `token-refill.exe` 的进程树：

```powershell
function Get-TreeProcess {
    param([int]$RootPid)
    $all = Get-CimInstance Win32_Process
    $result = @()
    $queue = [System.Collections.Generic.Queue[int]]::new()
    $queue.Enqueue($RootPid)
    while ($queue.Count -gt 0) {
        $parent = $queue.Dequeue()
        foreach ($child in ($all | Where-Object { $_.ParentProcessId -eq $parent })) {
            $result += $child
            $queue.Enqueue([int]$child.ProcessId)
        }
    }
    $result
}

$apps = Get-CimInstance Win32_Process |
    Where-Object { $_.ExecutablePath -like '*Token*Refill*' -or $_.Name -like '*token-refill*' }

if (-not $apps) { Write-Host 'Token Refill 未运行' }

foreach ($app in $apps) {
    $tree = Get-TreeProcess -RootPid $app.ProcessId
    $webviews = @($tree | Where-Object { $_.Name -eq 'msedgewebview2.exe' })
    $renderers = @($webviews | Where-Object { $_.CommandLine -match '--type=renderer' })
    $privateMb = [math]::Round((($tree | Measure-Object PrivatePageCount -Sum).Sum + $app.PrivatePageCount) / 1MB, 1)
    [pscustomobject]@{
        AppPid       = $app.ProcessId
        WebViewProcs = $webviews.Count
        Renderers    = $renderers.Count
        PrivateMB    = $privateMb
    } | Format-List
    $tree | Select-Object ProcessId, ParentProcessId, Name,
        @{n = 'PrivateMB'; e = { [math]::Round($_.PrivatePageCount / 1MB, 1) } },
        CommandLine | Format-Table -Wrap
}
```

`PrivatePageCount` 更接近任务管理器的「内存（活动专用工作集）」；`WorkingSetSize` 含共享页，不要直接对比。

### 预期 renderer 数量（只算本应用树）

| 状态 | 预期 |
| --- | --- |
| 启动，主窗口可见，面板未打开 | 1 |
| 面板打开 | 2 |
| 面板隐藏但未满 60 秒 | 2（尚未销毁） |
| 面板隐藏满 60 秒 | 1 |
| 主窗口关闭但未满 60 秒 | 1（面板未打开时） |
| 主窗口关闭满 60 秒，面板未打开 | 0 |
| 重新打开主窗口 | 1 |
| 再次打开面板 | 2 |

WebView2 可能在视图销毁后短暂保留进程，**等 15–30 秒再判定**；`WebViewProcs` 里还可能有 browser / GPU / utility 进程，
判断是否释放以 **renderer 数量**和 `PrivateMB` 为准。

> 如果测试时改过 exe 文件名（例如改成了 `tr.exe`），第 3 节脚本第一段的
> `ExecutablePath -like '*Token*Refill*'` 会匹配不到，请改成实际路径再运行。

## 4. 验证项

逐项填写第 5 节的结果表。建议先按顺序做 A → B → C → D，再做守卫和回归项。

### A. 启动与基线

**W-01 启动不预创建面板**
1. 退出所有 Token Refill 实例，按第 2 节启动并记录日志。
2. 等 10 秒，运行第 3 节脚本。
3. 预期：只有 1 个 `token-refill.exe`；renderer = 1；日志中没有 `tray-panel 加载耗时`。

### B. 菜单栏面板

**W-02 首次打开面板**
1. 左键点击菜单栏托盘图标。
2. 记录日志中的 `tray-panel 加载耗时 N ms` 与 renderer 数。
3. 预期：N 在 150–600 ms；面板出现在任务栏上方、水平对齐图标并夹在屏幕可用区域内；内容完整，无白屏/黑框；renderer = 2。

**W-03 加载期间重复点击**
1. 在面板出现前快速点两次托盘图标（或连续点三次）。
2. 预期：只出现一个面板，不重复创建；renderer 不会超过 2。

**W-04 面板隐藏 → 空闲销毁**
1. 点击面板外部或按 Esc 收起面板。
2. 60 秒内不要再点开，等待 65 秒。
3. 预期：日志出现 `销毁空闲窗口 tray-panel`；15 秒后 renderer 回到 1；应用进程仍在；托盘图标可响应。
4. 补充：在 60 秒内点开面板，应**不**触发销毁日志，面板即时出现（不重新加载）。

**W-05 面板销毁后重建**
1. 完成 W-04 后，再点一次托盘图标。
2. 预期：新的 `tray-panel 加载耗时 N ms`；面板内容正确（额度、更新时间、中转站列表）；位置正确；renderer = 2。

**W-06 面板里的“打开主窗口”**
1. 面板打开时点击底部「打开主窗口」。
2. 预期：面板收起，主窗口显示并聚焦；面板在 60 秒后按空闲策略销毁。

### C. 主窗口

**W-07 关闭主窗口 → 空闲销毁（关键回归）**
1. 关闭主窗口（标题栏 × 或 Alt+F4），确认图标只剩托盘、进程仍在。
2. 等 65 秒，期间不要打开面板或主窗口。
3. 预期：日志出现 `销毁空闲窗口 main`；renderer 回到 0（面板未打开时）；**`token-refill.exe` 仍在任务管理器中**；托盘图标仍可点击。
4. 若进程消失，说明 `ExitRequested` 拦截失败，按第 6 节收集信息。

**W-08 重建主窗口**
1. 托盘右键 →「打开主窗口」。
2. 或先打开面板，再点面板里的「打开主窗口」。
3. 或用同一个 exe 再启动一次（单实例应复用现有进程）。
4. 预期：日志出现 `主窗口重建加载耗时 N ms`（200–600 ms）；窗口出现并聚焦；账户与额度是最新状态；renderer = 1；进程数不增加。
5. 说明：重建后窗口回到屏幕中央、「总览」页签、滚动到顶部，这是预期行为。

### D. 常驻、退出与守卫

**W-09 显式退出**
1. 主窗口可见时：托盘右键 →「退出 Token Refill」。
2. 再启动，打开面板，点面板底部电源按钮退出。
3. 预期：两种方式都能立即结束进程；托盘图标消失；不会因为空闲销毁的退出拦截而“退不掉”。

**W-10 全部窗口销毁后仍可交互**
1. 主窗口关闭满 60 秒（W-07），面板也处于销毁状态。
2. 点托盘图标打开面板 → 能打开；再点托盘右键 →「打开主窗口」→ 能重建。
3. 预期：全程只有一个进程，功能正常。

**W-17 长时间驻留后的托盘响应（「菜单与主窗口打不开」回归）**
1. 主窗口关闭、面板关闭，全部窗口已按空闲策略销毁，放置 30 分钟以上（期间可锁屏、睡眠一次）。
2. 左键点托盘图标：面板应出现；再点一次应收起。
3. 右键 →「打开主窗口」：主窗口应出现并聚焦。
4. 重复第 2、3 步 3–5 次，中间分别等待一次 60 秒空闲销毁。
5. 预期：每次点击都出现 `[tray] 点击 Left/Down`、`Left/Up`；面板只在确实获得过焦点后失焦才收起；不出现 `创建 tray-panel 失败`、`仍不可见，重试显示`、`主窗口正在重建，忽略重复的打开请求` 连续堆积。
6. 若点击后**没有**任何 `[tray]` 日志，说明点击没到达事件循环（主线程被阻塞或托盘图标属于已退出的残留图标）：记录当时的日志文件与进程列表。

**W-11 登录守卫**
1. 账户页发起一次需要浏览器授权的登录，保持应用内登录弹窗打开。
2. 关闭主窗口（隐藏，不取消登录），等 65 秒。
3. 预期：**不出现** `销毁空闲窗口 main`；renderer 仍在；重新打开主窗口能看到登录弹窗并可继续/取消。
4. 取消或完成登录后，再等 65 秒，主窗口应正常销毁。

**W-12 更新下载守卫**（仅 NSIS 安装版，且必须存在更高版本）
1. 设置页点击「下载更新」，在下载过程中关闭主窗口，等 65 秒。
2. 预期：**不出现** `销毁空闲窗口 main`；下载完成后 60 秒再销毁。
3. 若下载太快无法观察，可跳过并记录“未覆盖”。

**W-13 后台刷新与通知**
1. 把刷新间隔设为 1 分钟，关闭所有窗口并等待销毁。
2. 保持应用驻留 5 分钟。
3. 预期：`%APPDATA%\dev.tokenrefill.desktop\state.json` 的 `revision` 持续递增（刷新仍在进行）；低额度/重置通知仍可送达；应用不会自己弹窗；renderer 保持 0。

### E. 平台回归

**W-14 多显示器与缩放**
1. 在 100% 和 150% 缩放的两块屏幕上分别点击托盘图标；把主窗口拖到副屏后关闭，再重建。
2. 预期：面板 380×560 逻辑点、贴任务栏上方、夹在可用区域内；主窗口重建后居中且尺寸正常，不出现模糊/裁切。

**W-15 注销与关机**
1. 在所有窗口都销毁、应用仅驻留托盘时注销或重启 Windows。
2. 预期：应用正常结束，不阻塞注销/关机，不残留 `token-refill.exe`。
3. 若注销被拖住或进程残留，记录并附上日志（说明退出拦截需要区分系统关机场景）。

**W-16 数据与凭证**
1. 验证前后检查 `%APPDATA%\dev.tokenrefill.desktop\`：`state.json`、`codex-profiles\`、`bailian-profiles\`、`api-profiles\`、`relay-profiles\` 结构不变。
2. 预期：没有新增凭证文件；`logs\window.log` 只有 `[app]` / `[tray]` / `[window]` 生命周期行，不含令牌、Cookie、邮箱或密钥。

### 通过标准

- W-01 至 W-10、W-13、W-14、W-16 全部通过；W-11、W-12 至少完成一项，另一项若因环境无法制造则标记「跳过」并说明原因。
- 任一场景出现以下情况即判失败：销毁窗口后 `token-refill.exe` 消失；面板/主窗口重建后空白或崩溃；显式退出无法结束进程；日志泄露凭证。
- 内存对比表中，本分支在「窗口都销毁」场景的 renderer 数应为 0，且 PrivateMB 明显低于旧版 0.2.6（具体数值以实测记录为准）。

## 5. 结果记录

| ID | 结果（通过/失败/跳过） | 观察值（耗时、renderer、PrivateMB） | 备注 |
| --- | --- | --- | --- |
| W-01 启动不预创建面板 |  |  |  |
| W-02 首次打开面板 |  |  |  |
| W-03 加载期间重复点击 |  |  |  |
| W-04 面板空闲销毁 |  |  |  |
| W-05 面板重建 |  |  |  |
| W-06 面板打开主窗口 |  |  |  |
| W-07 主窗口空闲销毁 |  |  |  |
| W-08 主窗口重建 |  |  |  |
| W-09 显式退出 |  |  |  |
| W-10 全部销毁后交互 |  |  |  |
| W-11 登录守卫 |  |  |  |
| W-12 更新下载守卫 |  |  |  |
| W-13 后台刷新与通知 |  |  |  |
| W-14 多显示器与缩放 |  |  |  |
| W-15 注销与关机 |  |  |  |
| W-16 数据与凭证 |  |  |  |
| W-17 长时间驻留后托盘响应 |  |  |  |

### 内存对比（同一台机器、相同账户）

| 场景 | 旧版 0.2.6（renderer / PrivateMB） | 本分支（renderer / PrivateMB） |
| --- | --- | --- |
| 启动 2 分钟后，主窗口关闭、面板关闭满 60 秒 |  |  |
| 面板打开 |  |  |
| 主窗口打开 |  |  |
| 面板打开 + 主窗口打开 |  |  |

预期：旧版启动即有 2 个 renderer（面板预创建）；本分支在窗口销毁后 renderer 为 0，PrivateMB 相应下降。
具体数值以实测为准，不在代码里写死。

## 6. 失败时收集

- `tr-verify.log`（完整日志）。
- 第 3 节脚本的输出（进程树、renderer、PrivateMB）。
- Windows 版本（`winver`）、WebView2 Runtime 版本：
  `Get-Item "$env:ProgramFiles(x86)\Microsoft\EdgeWebView\Application\*\msedgewebview2.exe" | Select-Object -Last 1 VersionInfo`
- 安装形态：NSIS 安装版 / 免安装 exe；账户数量与刷新间隔。
- 失败项的复现步骤，以及失败时的截图（任务管理器按应用分组 + 脚本输出）。