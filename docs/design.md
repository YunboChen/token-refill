# Token Refill 界面设计规范

- 文档版本：0.3
- 最近更新：2026-11-01
- 适用范围：管理窗口、菜单栏快速查看面板与桌面悬浮球前端（React + TypeScript）
- 事实来源：`src/App.module.css`、`src/styles.css`、`src/App.tsx`、`src/panels/`、`src/panels/TrayPanel.module.css`、`src/panels/FloatingBall.module.css`、`src/components/`、`src/lib/format.ts`、`src/lib/useDialogA11y.ts`
- 关联文档：[软件设计文档](./software-design.md)、[需求文档](./requirements.md)、[开发计划文档](./development-plan.md)

本文从现有实现提炼界面结构、组件用法与改造步骤，目标是让新增页面或重做相似页面时**先复用、再新增**，避免每次重画一套卡片、状态和弹窗。产品范围与业务规则以需求文档为准，本文只描述界面层。

## 0. 怎么用这份文档

| 要做的事 | 先看 |
| --- | --- |
| 新做一个侧栏页面 | §3 应用骨架、§5 页面模板、§7.1 |
| 重做一个类似列表/卡片页 | §4 组件目录、§5、§7.3 |
| 新增一个平台或账户类型 | §7.2 |
| 新增设置项或表单 | §4.5、§7.4 |
| 新增确认/授权弹窗 | §4.7、§7.5 |
| 查某个样式类是否已有 | §4 组件目录、§9 类名索引 |
| 避免重复踩坑 | §8 |

改造顺序建议：

1. 先判断页面属于哪种信息形态（分组卡片、网格对象、单列详情、设置列表）。
2. 选一个现有页面作为模板，复制其**外层骨架**而不是复制零散样式。
3. 只写页面独有的布局；通用尺寸、颜色、状态一律用现有 token 和类名。
4. 新样式确实无法复用时，先加到 `src/App.module.css` 对应分区，并在 §4 登记。

## 1. 设计原则

1. **安静的工具感**：界面服务于快速查看额度，不用大面积渐变、强投影或装饰性插画。
2. **三层容器**：画布（`--background`）→ 面板/卡片（`--surface`）→ 凹陷块（`--sunken`）。层级靠 1px 发丝线和底色差，不靠阴影；阴影只留给浮层弹窗。
3. **一种信息只用一个组件**：账户摘要、额度条、授权状态、错误提示都已有固定形态，新页面先找现成的，不再发明第二种表达。
4. **主色克制**：强调色只用于选中态、主按钮、可点击的强调信息。额度颜色统一为四级：正常用主色；剩余 ≤70% 且高于提醒阈值用 `--warning` 琥珀色表示「注意」；剩余 ≤提醒阈值用 `--danger` 红色；没有可靠比例用 `--muted` 灰色。查询失败与需要重新授权同样用红色。三个界面（主窗口、菜单栏面板、悬浮球）共用 `src/lib/quotaLevel.ts` 的 `quotaLevel()`，不要在页面里另写阈值判断。
5. **密度分级**：总览偏浏览（折叠摘要 + 展开详情），账户页偏管理（等宽网格 + 操作行），设置页偏逐项阅读（列表行 + 右侧控件）。
6. **交互可预期**：图标按钮必有 `aria-label` 和 `title`；刷新有旋转反馈；失败保留旧数据；浏览器预览下按钮禁用并说明原因。
7. **动效可有可无**：所有动画都遵循系统「减少动态效果」，不引入阻塞式动画。

## 2. 视觉基础

### 2.1 颜色 token

全部定义在 `src/styles.css` 的 `:root`，通过 `<html data-theme="light|dark|system">` 切换；`system` 由 `prefers-color-scheme` 媒体查询接管。

| Token | 浅色 | 深色 | 用途 |
| --- | --- | --- | --- |
| `--background` | `#f5f6f8` | `#0a0d11` | 窗口画布、页头、输入框底 |
| `--surface` | `#ffffff` | `#12171d` | 卡片、侧栏、弹窗 |
| `--sunken` | `#eceff3` | `#0c1015` | 凹陷块：标签、提示条、确认区、骨架 |
| `--hover` | `rgb(15 23 42 / 4%)` | `rgb(255 255 255 / 4.5%)` | 悬浮底色 |
| `--text` | `#14181f` | `#e7ecf3` | 主文字 |
| `--muted` | `#5a6472` | `#8d98a7` | 说明、次要信息、图标默认色 |
| `--border` | `#e3e7ed` | `#1e252e` | 发丝线、进度条轨道 |
| `--border-strong` | `#cbd3dd` | `#2b3440` | 悬浮描边、滚动条 |
| `--accent` | `#0b63ce` | `#79b8ff` | 强调文字、链接、选中图标 |
| `--accent-strong` | `#0f66d6` | `#3f8ff5` | 实色填充：主按钮、进度条、滑块 |
| `--accent-strong-hover` | `#0a57b8` | `#58a0ff` | 主按钮悬浮 |
| `--accent-text` | `#f7faff` | `#06121f` | 实色主色上的文字 |
| `--accent-soft` | `#e7eefb` | `#18293f` | 强调底色：选中导航、徽标、成功/进行中 |
| `--ok` / `--ok-soft` | `#167a4a` / `#e6f4ec` | `#4fbf82` / `#13281d` | 授权正常、成功 |
| `--warning` / `--warning-soft` | `#b06f00` / `#fdf1db` | `#f0b429` / `#332912` | 需要注意但未失败：剩余 ≤70% 且高于提醒阈值 |
| `--danger` / `--danger-soft` | `#b4233b` / `#fbeaed` | `#ff8b9c` / `#341c23` | 剩余 ≤提醒阈值、查询失败、破坏性操作 |
| `--focus` | `#0b63ce` | `#79b8ff` | 键盘焦点描边 |
| `--accent-ring` | 14% 主色 | 18% 主色 | 输入框聚焦光晕 |
| `--hud-accent` / `--hud-warning` / `--hud-danger` / `--hud-muted` | 引用 `--accent-strong` / `--warning` / `--danger` / `--muted` | 同左（解析为深色值） | 悬浮球额度环；跟随外观，保证与主窗口、菜单栏的额度层级一致 |
| `--ball-fill-start` / `--ball-fill-end` | `#ffffff` / `#e3e8ef` | `#1c2a3c` / `#0c121b` | 悬浮球球体径向渐变 |
| `--ball-border` / `--ball-shadow` | `rgb(15 23 42 / 22%)` / 顶部高光 + 底部内阴影 | `rgb(148 180 220 / 16%)` / 顶部高光 | 球体描边与内阴影 |
| `--ball-text` / `--ball-logo` / `--ball-detail` | `#14181f` / `#2b3440` / `#5a6472` | `#eef4ff` / `#dbe8fb` / `#93a4ba` | 球内百分比、平台标识与周期文字 |
| `--ball-track` / `--ball-spinner` | `rgb(15 23 42 / 14%)` / `rgb(15 23 42 / 42%)` | `rgb(255 255 255 / 13%)` / `rgb(255 255 255 / 42%)` | 额度环轨道与刷新指示 |

使用规则：

- **文字强调用 `--accent`，实色块用 `--accent-strong`**，不要反过来。
- 新颜色先问是否能用现有语义 token；确需新增时同时补浅色、深色和系统跟随三处。
- 不在组件里写十六进制色值（弹窗遮罩、开关滑块等高光/中性黑除外）。
- 悬浮球跟随主题：球体用 `--ball-*`（浅色白底深字、深色保留深蓝黑球体），额度环用引用语义色的 `--hud-*`；球内不要再直接写十六进制。

### 2.2 圆角与层级

| Token / 值 | 用途 |
| --- | --- |
| `--radius-card` = 14px | 卡片、弹窗、空态、额度板 |
| `--radius-control` = 9px | 输入框、按钮、提示块、导航项 |
| `--radius-pill` = 999px | 标签、开关、额度条、圆点状态 |
| 10–12px | 账户/平台徽标、空态图标底板 |
| 5–8px | 药丸滑块、弹窗关闭按钮、代码片、授权链接 |

层级只用三种手段：底色差、1px 发丝线、留白。**卡片不再套卡片**；卡内分区用 `.planRow + .planRow` 这类相邻发丝线处理。唯一使用阴影的地方是 `.dialog`。

### 2.3 字体与字号

字体栈在 `:root` 定义：系统 UI 字体（SF Pro / PingFang SC / Segoe UI），等宽用 `--font-mono`。中文文案为主，西文与数字跟随系统。

| 角色 | 字号 / 字重 | 类名 |
| --- | --- | --- |
| 页面标题 | 27px / 600，`-0.022em` | `.panelTitle` |
| 弹窗标题 | 20px / 600 | `.dialog h2` |
| 分组标题（设置） | 14px / 600 | `.groupTitle` |
| 账户名 / 平台名（完整尺寸） | 15.5px / 15px / 600 | `.accountName`、`.quotaAccountName`、`.platformName` |
| 正文说明 | 12.5px / 400 | `.subtitle`、`.usageNote`、`.settingHint` |
| 次级说明 | 11–12px / 400 | `.accountMeta`、`.quotaMeta`、`.platformDescription` |
| 关键数值（额度百分比） | 21px / 600，等宽数字 | `.quotaPercent` |
| 金额（中转站） | 28px / 600 | `.relayAmount` |
| 标签 / 状态 | 10.5–11.5px / 500–600 | `.planTier`、`.authStatus`、`.updatedAt` |
| 代码 / 地址 | `--font-mono` 11–13px | `.codeChip`、`.authUrl` |

数字类信息统一加 `font-variant-numeric: tabular-nums`，避免刷新时宽度跳动。长文本一律 `min-width: 0` + `text-overflow: ellipsis`，不靠换行撑高卡片。

### 2.4 间距与尺寸

| 位置 | 值 |
| --- | --- |
| 侧栏宽度 | 212px |
| 页头内边距 | `22px 28px 18px` |
| 内容滚动区内边距 | `20px 28px 32px` |
| 正文最大宽度 | 920px，水平居中 |
| 平台分组间距 | 组间 26px；分组之间自动加 24px 上内边距 + 发丝线 |
| 卡片内边距 | 18px；紧凑卡 `15px 16px 12px`；总览额度板内账户 `13px 18px 16px` |
| 卡片间距 | 同类卡片 10px；卡片与标题 14–16px |
| 控件高度 | 标准按钮 34px；弹窗按钮 36px；小按钮 26–30px；输入框 34px（弹窗 38px） |
| 图标尺寸 | 导航 17px；按钮内 13–16px；平台图形 18px（分组头 20px）；空态 24px |
| 菜单栏面板 | 380×560 点；圆角 13px；内容区内边距 11–12px；水平居中于菜单栏图标 |

### 2.5 动效

全局在 `src/main.tsx` 配置：`LazyMotion + domAnimation`、`MotionConfig reducedMotion="user"`，组件使用 `m.*` 而不是 `motion.*`。

| 场景 | 参数 | 位置 |
| --- | --- | --- |
| 页面切换进场 | 220ms，`cubic-bezier(0.16, 1, 0.3, 1)`，透明度 + 10px 上浮 | `.panelWrap` / `panelIn` |
| 账户展开收起 | 260ms，同曲线，`height: auto` + 透明度 | `OverviewPanel` 的 `AnimatePresence` |
| 额度条填充 | 700ms，同曲线 | `OverviewPanel` 的 `QuotaBar` |
| 弹窗进场 | spring `stiffness 460 / damping 34`，缩放 0.96 + 上移 10px | `AccountsPanel` |
| 弹窗退场 | 300ms `easeIn`；遮罩 340ms | 同上 |
| 文字/底色悬浮 | 150ms `ease-out` | 全站 |
| 按钮按下 | `transform: scale(0.97–0.98)`，100ms | `.button`、`.miniButton` 等 |
| 旋转指示 | 900ms linear infinite | `.spin` |

使用 `useReducedMotion()` 时把动画时长设为 0；CSS 侧另有全局 `prefers-reduced-motion` 兜底。新增动画必须同时处理这两层。

### 2.6 图标与品牌图形

- 图标统一用 `@phosphor-icons/react`，尺寸通过 `size` 传入，装饰性图标加 `aria-hidden`。
- 平台品牌图形在 `src/components/PlatformLogo.tsx`：Codex / OpenCode / Command Code 用单色路径跟随主题；百炼的立方体与 Antigravity 的 A 形本身是多色标识，保留品牌原色。
- 没有品牌图形时回退到平台单字标记（`platformMeta().mark`）；账户徽标用账户名首字符（`accountMark()`）。
- 字标 `src/assets/logo-text.png`，深色模式通过 `--wordmark-filter` 提亮。

## 3. 应用骨架

```
┌──────────────┬────────────────────────────────────────────┐
│ 侧栏 212px    │ 页头（固定，不滚动，可拖拽窗口）              │
│  - 拖拽安全区 │  标题 27px + 一行副标题                     │
│  - 字标       ├────────────────────────────────────────────┤
│  - 主导航     │ 滚动区 mainScroll（max-width 920 居中）      │
│  - 版本/更新  │  全局错误条（可选）                          │
│              │  panelWrap（切换时 220ms 进场）              │
│              │  浏览器预览页脚（可选）                       │
└──────────────┴────────────────────────────────────────────┘
```

### 3.1 壳层结构

- `src/App.tsx` 管理 `active` 与 `PANELS` 定义；四个页面只负责内容，不重复画页头。
- 页面元信息集中在 `PANELS`：`key / label / Icon / title / subtitle`。标题与副标题由壳层统一渲染，页面内不再写大标题。
- 导航使用 `role="tablist"` + `role="tab"` + `aria-selected` + `aria-controls`；内容区使用 `role="tabpanel"` 并通过 `key={active}` 触发进场动画。
- 全局 `error` 显示在滚动区顶部（`.noticeDanger .globalNotice`）；页面级错误留在各自页面内。
- 根层 `height: 100dvh; overflow: hidden`，**只有 `.mainScroll` 滚动**，避免 macOS WKWebView 把固定侧栏横向拖动。

### 3.2 标题栏与平台适配

- macOS 使用 Overlay 原生标题栏：`.titlebarSafeArea` 高 44px，承载红绿灯并作为拖拽区；不渲染 `TitleBar` 组件。
- Windows 关闭系统装饰（Rust `setup()` 中 `set_decorations(false)`），由 `TitleBar` 自绘标题栏，负责拖拽、最小化、最大化与关闭；`.app[data-platform='windows']` 在非最大化时补 1px 内描边。
- 主窗口在 `tauri.conf.json` 中以 `visible: false` 创建，避免 React 首绘前露出系统标题栏：macOS 在 `setup()` 中直接显示；Windows 由 `TitleBar` 首帧 emit `main-ui-ready`，Rust 侧收到后 `show()` 并聚焦，另有 5 秒兜底。
- 页头、标题、副标题均带 `data-tauri-drag-region`；按钮和可交互元素不要加，否则会吞掉点击。
- 平台信息来自 `getAppInfo()`；首次渲染用 UA 同步兜底（避免 Windows 标题栏首帧缺失），读取失败保持 macOS 布局，不阻塞界面。

### 3.3 新增页面的接入点

新增一个侧栏页面需要改三处，缺一不可：

1. `PanelKey` 联合类型。
2. `PANELS` 数组（顺序即导航顺序，同时决定标题/副标题）。
3. 内容区的条件渲染分支。

页面文件放在 `src/panels/`，默认导出组件，外层用 `<section>`，内容首块不要自带顶部 margin（`.panelWrap > section > :first-child` 已处理）。

## 4. 组件目录

所有样式集中在 `src/App.module.css`，各文件通过 `import styles from '../App.module.css'` 使用。没有按组件拆分的 CSS Module；新增类请沿用「语义名 + `data-*` 状态」的写法。

### 4.1 容器与布局

| 类名 | 用途 | 用法与变体 |
| --- | --- | --- |
| `.card` | 通用内容卡 | `margin-top: 16px`、内边距 18px、发丝线、14px 圆角。用于表单、说明、单独内容块 |
| `.card.listCard` | 列表型设置卡 | 内边距收成 `2px 14px`，子项之间自动加发丝线；配合 `.settingRow` / `.settingRowStatic` |
| `.overviewStack` | 页面级纵向堆叠 | `gap: 26px`，相邻块自动加 24px 上内边距 + 通栏发丝线；总览、中转站使用 |
| `.platformStack` | 平台分组堆叠 | 同上节奏，账户页使用 |
| `.platformGroup` | 单个平台分组容器 | `min-width: 0`；总览、账户、待接入平台共用，不再按页面拆同义类 |
| `.platformSectionHead` | 压缩版平台标题行 | 把 `PlatformHeading` 的徽标收到 20px、去底色、名称降到 13px |
| `.accountGrid` | 账户卡网格 | `repeat(auto-fill, minmax(min(100%, 250px), 1fr))`，`gap: 10px`；默认两列，宽窗三列，窄窗一列 |
| `.accountStack` | 单列卡片列表 | `gap: 10px`；总览的中转站额度标签页使用 |
| `.quotaBoard` | 同平台账户合集卡 | 外层一张 14px 圆角卡，内部账户之间用发丝线；总览使用 |
| `.mainScroll` | 内容滚动容器 | 只有这里滚动；`overscroll-behavior: none` |
| `.panelWrap` | 页面进场包装 | 切换页面时 `key` 变化触发 220ms 进场 |

### 4.2 页头、分组标题与标签

| 类名 / 组件 | 用途 | 用法与变体 |
| --- | --- | --- |
| `.panelTitle` / `.subtitle` | 壳层页头 | 由 `App.tsx` 统一渲染；副标题单行省略，保证页头高度稳定 |
| `.groupTitle` | 设置页分组标题 | 直接作为 `h2`，与下方 `.card.listCard` 通过 `aria-labelledby` 关联 |
| `<PlatformHeading>` | 平台分组标题 | Props：`platform`、`count`、`status`、`description`、`action`、`active`；总览与账户页共用 |
| `<PlatformLogo>` | 平台品牌图形 | 无图形时回退单字；单色图形继承父级 `currentColor`，多色品牌保留原色 |
| `.platformBadge` | 平台徽标底板 | 36px、10px 圆角；`data-active` 时用强调底色 |
| `.platformName` / `.platformVendor` / `.platformDescription` | 平台文字块 | 名称与厂商同排，说明单行省略 |
| `.platformAside` / `.platformCount` / `.platformStatus` | 标题右侧信息 | 账户数、待接入状态、操作按钮 |
| `.planTier` | 套餐档位小标签 | 中性胶囊，10.5px；总览眉线、账户卡、折叠摘要共用 |
| `.planTag` | 中性标签 | 10.5px 胶囊，中转站来源/平台名/额度模式共用；平台名不再使用强调色 |

### 4.3 账户与额度展示

现有三种账户卡形态，**新页面按信息密度选一种，不要混用**：

| 形态 | 类名 | 适合场景 | 参考页面 |
| --- | --- | --- | --- |
| 可折叠额度板 | `.quotaBoard` + `.quotaAccount*` + `.metric*` | 同平台多账户，默认只看摘要，展开看完整窗口 | 总览 |
| 管理网格卡 | `.accountGrid` + `.accountTile*` | 需要并排比较、每张卡都有独立操作 | 账户、中转站账户管理 |
| 单列详情卡 | `.accountStack` + `.accountCard*` | 每个对象信息较多、按时间或来源排列 | 总览的中转站额度 |

总览额度板关键结构：

| 类名 | 作用 |
| --- | --- |
| `.quotaAccount` | 单个账户；收起与展开同一套 18/20/20 内边距，与中转站卡一致 |
| `.quotaAccountHead` / `.quotaAccountToggle` | 整行可点的展开按钮，含名称、档位、折叠摘要 |
| `.quotaAccountInfo` / `.quotaAccountIdentity` / `.quotaAccountName` / `.quotaAccountMeta` / `.quotaSummary` | 左侧常驻两行：名称（含档位）+ 身份/来源；摘要仅收起时出现，与整块垂直居中、位于右侧并留出间距，单行省略 |
| `.quotaAccountTools` / `.chevronButton` / `.chevronIcon` | 更新时间、刷新按钮、箭头；箭头只是视觉补充，键盘操作由左侧按钮承担 |
| `.quotaBody` / `.quotaBodyInner` | 折叠动画的裁剪层与内层留白 |
| `.quotaAccountMeta` / `.metaDot` | 标题区第二行的身份说明与分隔点（展开/收起都在） |
| `.metricGrid` / `.metric` | 横向排布的额度窗口；条目少时平分宽度 |
| `.metricDivider` | 窗口间竖线；`data-strong` 表示套餐分界，竖线占满整行高度 |
| `.metricEyebrow` / `.metricProduct` | 多套餐时的眉线，固定高度保证下方标签对齐 |
| `.metricLabel` / `.metricMeta` / `.metricMetaPart` / `.metricDot` | 窗口名、已用/重置说明；`.metricMeta` 固定分成两行（已用在上、重置时间在下），与窗口列数无关；窗口宽度 ≤1050px 时隐藏 `.metricDot` |
| `.quotaBarRow` / `.quotaPercent` / `.quotaBarTrack` / `.quotaBarFill` | 百分比 + 进度条；`data-level="warning|danger|unknown"` 按统一额度等级着色，`normal` 保持中性 |
| `.quotaLowNote` | 达到提醒阈值的红色说明；剩余为 0 时显示「额度已用尽」 |
| `.dataGaps` | 数据缺口说明 |

账户管理网格卡关键结构：

| 类名 | 作用 |
| --- | --- |
| `.accountTile` | 网格单元，纵向三段：身份、套餐、操作；`margin-top: auto` 让操作行吸底 |
| `.accountIdentityCell` / `.accountMark` | 首字符徽标 + 身份区 |
| `.accountIdentity` / `.accountNameRow` / `.accountName` | 名称与授权状态同排，名称省略、状态不换行 |
| `.accountMeta` | 接入方式与脱敏身份提示 |
| `.planPanel` / `.planRow` | 套餐行；`data-inactive` 弱化未开通套餐 |
| `.planRowMain` / `.planRowName` / `.planTier` | 套餐名与档位 |
| `.planRowState` | 查询状态；`data-state="busy"` 主色、`"error"` 危险色 |
| `.accountFoot` / `.accountRefresh` | 吸底操作行；刷新带文字标签 |
| `.accountActions` / `.miniButton` | 账户级图标操作；破坏性操作用 `data-danger` |
| `.accountAddTile` / `.accountAddIcon` / `.accountAddLabel` | 网格末尾的虚线添加入口，至少 118px 高 |
| `.accountEmptyNote` | 账户没有套餐时的行内说明 |

中转站额度卡（总览）关键结构：

| 类名 | 作用 |
| --- | --- |
| `.accountCard` / `.accountHead` / `.accountTitleBlock` | 单列额度卡与标题区 |
| `.accountMetaLine` / `.accountHint` / `.planTag` | 来源、平台、额度模式、密钥名等标签行；平台类型与来源、额度模式同用中性标签 |
| `.accountTools` / `.refreshButton` / `.updatedAt` / `.chevronButton` | 右侧刷新、更新时间与展开箭头 |
| `.relayQuotaGrid` / `.relayQuotaCell` | 「密钥额度 / 账户额度」两列；竖线分隔，窄窗口退成单列 |
| `.relayQuotaLabel` / `.relayQuotaCaption` / `.relayAmount` | 单元格标签、额度口径与金额；未知金额用 `data-muted` 弱化 |
| `.relayMeterRow` / `.relayMeterValue` / `.relayBar` / `.relayBarFill` | 剩余比例条与数值；`data-level` 按统一额度等级着色 |
| `.relayWindows` / `.relayWindow` / `.relayWindowHead` / `.relaySectionLabel` | 密钥限速窗口与账户订阅窗口 |

中转站管理卡（管理页）沿用账户网格卡：

| 类名 | 作用 |
| --- | --- |
| `.accountGrid` / `.accountTile` | 与账户页同一套网格卡；身份、状态、操作三段 |
| `.accountIdentityCell` / `.accountMark` | 插头徽标 + host 身份区 |
| `.relayTileStatus` / `.relayTileMeta` / `.relayTileNotice` | 额度同步状态、凭证与访问令牌说明、刷新失败提示（最多两行） |
| `.accountAddTile` | 网格末尾的虚线添加入口，打开添加弹窗 |
| `.relayScanButton` | 平台标题右侧的「扫描本地配置」动作，打开候选勾选弹窗 |
| `.relayCandidateList` / `.relayCandidate` | 扫描弹窗中的候选列表：复选框、来源、provider 名称与地址 |
| `.relaySkipped` / `.relaySkippedToggle` | 未识别的本地配置：默认收起，展开后才列出来源、地址与原因 |

### 4.4 状态与提示

按「错误影响范围」选类名，不要一律用红条：

| 类名 | 语义 | 位置建议 |
| --- | --- | --- |
| `.noticeDanger` | 阻断性错误 | 全局错误条加 `.globalNotice`；页面级加 `.accountNotice`；卡片内直接使用 |
| `.noticeStale` | 刷新失败但仍有旧数据 | 卡片内，中性底色，不抢已有数据 |
| `.noticeAside` | 错误码等次要信息 | 嵌在提示条内，降低透明度 |
| `.quotaNotices` | 一组额度提示 | 总览展开区顶部 |
| `.authStatus` + `.authDot` | 授权状态 | `data-state="ok"` 绿色，`"needs-reauth"` 红色，未知态中性 |
| `.planRowState` | 套餐查询状态 | 状态键由 `planStateKey()` 提供（`inactive / busy / error / ok / idle`）；CSS 只为 `busy` 和 `error` 上色 |
| `.updatedAt` | 最近刷新时间 | 卡片标题右侧，等宽数字 |
| `.toast` / `.toastSuccess` | 瞬时提示 toast | 固定窗口底部居中，默认 `--danger-soft` 底色，成功时换 `--ok-soft`；操作级失败与导入成功都用 `Toast` 自动消失 |
| `.overviewEmpty` / `.emptyGlyph` / `.emptyTitle` | 整页空态 | 没有数据时替代列表 |
| `.noticeStale` 与 `.accountNote` | 行内补充说明 | 放在数据区下方，字号 12px |

文案约定：错误提示先说明「哪个对象失败了」，再给原因；有旧数据时明确「当前显示 X 时的数据」；不要用 0 或「—」伪装失败结果。

### 4.5 表单控件

| 类名 | 用途 | 关键约定 |
| --- | --- | --- |
| `.field` / `.fieldLabel` / `.textInput` | 标准输入组 | label 包住控件；输入框最小高 34px；聚焦用主色描边 + 3px 光晕 |
| `.textInput`（弹窗内） | 放大输入框 | `.dialog .textInput`：38px 高、`--sunken` 底 |
| `.pills` / `.pill` / `.pillThumb` / `.pillText` | 分段单选 | 原生 `radio` 隐藏但保留键盘语义；外层传 `--pill-index`、`--pill-count`；放在 `.settingRowStatic` 内时由 CSS 强制等宽列 |
| `.switchWrap` / `.switchInput` / `.switchTrack` / `.switchKnob` | 开关 | `input type="checkbox" role="switch"`；用 `aria-labelledby` / `aria-describedby` 关联文字 |
| `.settingRow` | 整行可点的设置项 | `label` 包住文字与开关，悬浮整行变色 |
| `.settingRowStatic` | 控件自带语义的设置项 | 文字在左、控件在右；窄窗可退回上下布局 |
| `.stackRow` | 纵向设置块 | 阈值滑块等；`data-disabled` 弱化子项 |
| `.range` / `.rangeHead` / `.rangeValue` / `.rangeScale` | 滑块 | 通过 `--range-fill` 画已填充轨道；同时写 WebKit 与 Firefox 伪元素 |
| `.settingText` / `.settingLabel` / `.settingHint` | 设置文字层级 | 标题 13px/600，说明 12px 次要色；危险说明加 `data-danger` |

敏感输入（如中转站 API Key、New API 访问令牌）应使用**非受控 ref**读取，不进入 React 状态、日志或 `state.json`；手动凭证由 Rust 写入 `relay-profiles` 私有目录。现有 `RelayPanel` 是范例。

### 4.6 按钮

| 类名 | 层级 | 尺寸 / 变体 |
| --- | --- | --- |
| `.button` | 主操作 | 实色主色，34px 高；弹窗内 36px |
| `.buttonGhost` | 次操作 | 描边 + `--surface` 底；与主按钮成对出现在弹窗右侧 |
| `.dangerButton` | 破坏性确认 | 红描边 + 红底，用于移除确认 |
| `.testButton` | 设置页动作 | 30px 高、12px 字；`data-primary` 时用强调底色 |
| `.miniButton` | 图标操作 | 26px 方形；`data-danger` 悬浮转红；必须配 `aria-label` + `title` |
| `.refreshButton` | 圆形刷新 | 28px；旋转图标用 `.spin`；禁用时降到 50% |
| `.accountRefresh` | 带文字的轻量刷新 | 26px 高、11.5px 字，账户卡底部使用 |
| `.accountAddTile` | 添加入口 | 虚线卡片，悬浮转实线主色；与账户卡同网格 |
| `.updateChip` | 侧栏更新提示 | 胶囊，悬浮反转为主色实底 |
| `.buttonRow` | 按钮并排 | 设置页版本检查与更新使用 |
| `.updateProgress` | 更新下载进度条 | 细轨道 + accent 填充，下载中/已下载时显示 |

按钮状态统一：`hover` 只改底色/描边，`active` 轻微缩放，`disabled` 降透明度 + `cursor: not-allowed`。所有图标按钮都要有可读文本（`aria-label`），不能只靠图形。

### 4.7 弹窗

| 类名 | 作用 |
| --- | --- |
| `.dialogBackdrop` | 固定全屏遮罩，`z-index: 50`，居中弹窗 |
| `.dialog` | 460px 宽、20px 内边距、14px 圆角、唯一使用大阴影的容器 |
| `.dialogClose` | 右上角关闭按钮，绝对定位 |
| `.dialogHeader` / `.dialogSubtitle` | 标题 + 一行说明，右侧预留关闭按钮空间 |
| `.dialogSteps` / `.dialogStep` / `.stepMarker` / `.stepBody` / `.stepTitle` / `.stepText` | 编号步骤 + 竖向连线，讲授权流程 |
| `.dialogFieldGroup` / `.dialogNote` | 输入与隐私说明成组 |
| `.authBlock` / `.authUrl` / `.loginStatus` | 授权链接与等待状态 |
| `.dialogSuccess` / `.dialogSuccessIcon` / `.dialogSuccessBody` / `.dialogSuccessTitle` / `.dialogSuccessText` | 成功态：结论 + 后续动作 |
| `.dialogActions` | 右对齐按钮组；弹窗内按钮统一 36px 高 |
| `.dialogForm` | 纯表单弹窗（重命名） |

硬性约定：

- 弹窗必须用 `createPortal(..., document.body)`。面板容器带进场 `transform`，会成为 `position: fixed` 后代的包含块，就地渲染会让遮罩只盖住内容区。
- 用 `AnimatePresence` 包住遮罩与弹窗；遮罩淡入 160ms、淡出 340ms，弹窗进入用 spring，退场用 300ms 缓入淡出。
- 键盘行为统一由 `useDialogA11y` 提供：打开前用 `captureTrigger(触发元素)` 记录入口；打开时焦点落在弹窗容器（已有 `autoFocus` 输入框时保持不动），Tab / Shift+Tab 在弹窗内循环，Esc 关闭，关闭后焦点回到触发元素。
- 遮罩点击关闭要判断 `event.target === event.currentTarget`，避免点内容区误关。
- 标题必须有唯一 `id`，弹窗用 `aria-labelledby` 关联；`role="dialog"` + `aria-modal="true"`。
- 一个弹窗只做一件事：添加/授权、重命名、移除分别建模，不用一个弹窗塞多种表单。
- 授权流程关闭时必须显式取消后台流程，并作废尚未返回的启动请求（见 `AccountsPanel` 的 `startAttemptRef` 与 `pendingLoginId`）。

### 4.8 空态与加载

| 类名 | 用途 |
| --- | --- |
| `.overviewEmpty` + `.emptyGlyph` + `.emptyTitle` + `.usageNote` | 整块空态：图标、一句结论、一条下一步说明 |
| `.accountNote` | 行内空态/说明，跟随卡片内容 |
| `.skel` / `.skelLabel` / `.skelValue` / `.skelBar` / `.skelMeta` | 与额度窗口同形的骨架屏，`pulse` 1.3s |
| `.spin` | 图标旋转，表示刷新/等待 |

加载策略：

- **首次加载**用骨架屏（`UsageSkeleton`），保持最终布局形状。
- **刷新已有数据**保留旧值，只在刷新按钮上旋转，并在 `updatedAt` 后追加「· 刷新中」。
- **等待授权**用 `.loginStatus` + 旋转图标，不整屏遮罩。

## 5. 页面结构模板

### 5.1 总览（`OverviewPanel.tsx`）

```
section
├─ overviewTabs（订阅账户额度 / 中转站账户额度）
├─ [订阅账户额度] tabpanel
│  └─ overviewStack
│     ├─ 空态（无账户）overviewEmpty
│     └─ Reorder.Group（平台可拖动排序）
│        └─ 每个平台 platformGroup（Reorder.Item）
│           ├─ platformSectionHead → PlatformHeading（右侧排序手柄）
│           └─ quotaBoard（账户 Reorder.Group）
│              ├─ quotaAccount（展开：第一个账户，工具区含排序手柄）
│              │  ├─ quotaAccountHead（名称/档位 + 身份/来源 + 刷新 + 箭头）
│              │  └─ quotaBody > quotaBodyInner
│              │     ├─ 提示（stale / danger）
│              │     ├─ metricGrid（窗口 / 分隔线）
│              │     └─ dataGaps
│              └─ quotaAccount（折叠：其余账户，同一头部 + 一行摘要）
└─ [中转站账户额度] tabpanel
   └─ RelayQuotaBoard
      ├─ platformSectionHead（中转站账户 · 账户余额与密钥额度 · N 个账户）
      └─ accountStack（Reorder.Group）
         └─ accountCard × N（默认展开，可收起；Reorder.Item，工具区含排序手柄）
            ├─ accountHead（host + 类型/来源/额度口径 + 刷新/箭头；工具与两行标题垂直居中）
            │  └─ 收起时左侧保持两行身份信息，中间显示简易额度摘要，右侧只剩刷新和箭头
            └─ quotaBody → relayQuotaGrid
               ├─ relayQuotaCell（密钥额度：金额、到期时间、限速窗口）
               └─ relayQuotaCell（账户额度：钱包/订阅/账户余额、比例条、订阅窗口）
```

关键行为：

- 标签栏沿用设置页分段控件的视觉语言：独立使用 `.overviewTabs` / `.overviewTab`，但复用 `.pillThumb` 滑块，等宽列 + `--pill-index` / `--pill-count` 驱动滑动；使用 `role="tablist"` / `role="tab"`；默认停留在「订阅账户额度」，切换不改变顶部页头。
- 中转站账户与「中转站账户管理」页共用 `RelayProvider` 状态：进入任一页面时只加载已保存条目与上次成功快照，并把超过刷新间隔的账户自动刷新一次；扫描本地配置是管理页上的手动入口，之后在任一页面手动刷新都会同步。
- 平台与账户都支持拖动排序：平台标题右侧和账户工具区各有排序手柄（`ReorderHandle`），拖动由 motion 的 `Reorder` 接管，只从手柄开始，不影响账户展开、刷新与文本选择；手柄聚焦后可用上下方向键移动一格。同平台只有一个账户时不显示账户手柄。
- 排序保存到 Rust 状态的 `overviewOrder`（平台键与账户标识，不含名称或凭证），每次打开都按保存后的顺序展示；顺序同样作用于菜单栏快速面板。未参与过排序的新平台/新账户保持默认顺序并排在已排序项之后。界面先本地生效再保存，保存失败时回到后端仍持有的顺序。
- 菜单栏面板的中转站分组与平台分组共用 `overviewOrder.platforms`，用固定标识 `relay` 记录位置；总览页只排列平台分组，保存时会把这个附加标识按原来的槽位保留，不会因为一次拖动把中转站重置到末尾。
- 中转站额度页的账户使用同一套手柄与本地优先保存逻辑，顺序保存在 `relayOrder`（只含中转站注册表标识）；菜单栏面板的中转站分组同样按该顺序展示，管理页仍保持注册表顺序。列表只有一个账户时不显示手柄。
- 平台顺序默认仍由 `groupAccountsByPlatform()` 的固定顺序兜底；用户保存排序后优先使用保存顺序，账户增删不会让未参与排序的分组跳动。
- 同一平台的账户共用一张卡；默认展开第一个，`undefined` 表示沿用默认，`null` 表示全部收起。
- 折叠摘要由 `accountSummary()` 生成：单套餐列各窗口余量；多套餐（如百炼的 Token Plan + Coding Plan）每个套餐只列剩余最低的窗口，避免摘要铺满整行。
- 标题区固定为两行：第一行名称与档位，第二行身份/来源；收起时右侧再出现额度摘要（与整块垂直居中、单行省略），左侧身份列沿用 132px 起步宽度。
- 展开与收起共用同一个头部，第二行身份/来源常驻（与中转站卡一致），只对额度区做高度动画，避免整行跳动。
- `buildMetrics()` 把「套餐 → 额度组 → 窗口」拍平成格子；多套餐时显示眉线，单类套餐内有多个额度组时显示组名。
- 展开体只渲染在 `open` 时，折叠时 DOM 不保留内容。
- 中转站额度页对金额、比例、到期时间与窗口额度都做了「未知/不限额」分支，不把未知显示成 0；管理页的刷新与总览共用 `refreshingIds`，批量刷新时可同时显示多个。
- 中转站额度卡默认全部展开，箭头只记录用户主动收起的账户；收起后左侧仍保留两行身份/口径信息，右侧显示「密钥 / 钱包或账户」的简易金额摘要，与订阅额度的折叠密度一致。页头「N 个账户」与分组标题跟随列表长度变化。
- 中转站卡片的头部与两列内容之间只留间距，不画通栏分隔线，与订阅额度卡的展开体保持一致；两列之间仍有竖向发丝线。

### 5.2 账户（`AccountsPanel.tsx`）

```
section
├─ platformStack
│  ├─ 每个已开放平台 platformGroup
│  │  ├─ platformSectionHead → PlatformHeading（带账户数）
│  │  └─ accountGrid
│  │     ├─ accountTile × N
│  │     └─ accountAddTile（平台已开放时）
│  └─ 每个待接入平台 platformGroup（标题 + plannedNote）
└─ portal → 弹窗（add-form / login / rename / remove）
```

关键行为：

- `accountPlatformGroups()` 会为已开放但尚无账户的平台保留空分组，保证每个平台都有自己的添加入口。
- 账户卡只列已开通套餐；全部未开通时保留列表并标记「未开通」。
- 弹窗是状态联合类型 `DialogState`，授权流程还包含 `phase`：`waiting / succeeded / failed / canceled`。
- 授权事件由 `onAccountLoginChanged` 驱动，只携带脱敏状态；离开页面或关闭弹窗要取消后台流程。
- 添加流程的平台由入口决定，弹窗内不再切换平台。
- 授权没能启动（例如本机没有 Codex 客户端）、API Key 校验失败等操作级错误统一用窗口底部的 `Toast`（`tone="danger"`）提示（4 秒自动消失），不占弹窗和页面的布局；添加弹窗保持打开，方便改完重试或取消。

### 5.3 中转站账户管理（`RelayPanel.tsx`）

```
section
├─ 页面级错误 noticeDanger（没有弹窗时）
└─ platformGroup
   ├─ platformSectionHead（插头徽标 + 中转站账户 + 说明 + 账户数 + 扫描本地配置）
   ├─ accountGrid
   │  ├─ accountTile × N（host、平台标签、来源/凭证/令牌、额度状态、刷新/令牌/移除）
   │  └─ accountAddTile（虚线添加入口，打开添加弹窗）
   ├─ 空列表说明 / 操作提示
   ├─ relaySkipped（默认收起：未识别的本地配置）
   └─ 浏览器预览说明
└─ 弹窗（portal）：添加 / 扫描本地配置 / 补充访问令牌 / 移除
```

关键行为：

- 本页只负责账户与凭证管理；额度金额、比例、窗口与到期时间在总览的「中转站账户额度」展示。管理卡只保留「额度已同步 / 密钥不可用 / 尚未获取额度」与更新时间。
- 进入页面时若尚未加载，只读取已保存条目与上次成功快照；标题右侧「扫描本地配置」打开候选弹窗，扫描只列出来源、provider 名称、地址与「已在列表中」状态，不发起网络请求。默认勾选尚未添加的候选，用户确认「导入所选」后才查询额度并加入列表。导入的条目只保存来源引用，启动与刷新时重新读取源配置，卡片显示「来源已保存 · 启动与刷新时重新读取」。数据与总览共用 `RelayProvider`。
- 添加、补充访问令牌与移除统一用弹窗，和账户页保持一致；添加位是网格末尾的虚线卡。
- API Key 与访问令牌都用 ref 读取，不进入组件状态；保存动作由 Rust 完成。
- New API 卡片提供访问令牌入口；Sub2API 不显示该入口。
- 未识别的本地配置默认收起，避免说明文字占满首屏。

### 5.4 设置（`SettingsPanel.tsx`）

```
section
├─ groupTitle「通用」+ card.listCard
│  ├─ settingRowStatic（外观 + 分段单选）
│  └─ Toggle（开机自启）
├─ groupTitle「刷新」+ card.listCard
│  └─ settingRowStatic（自动刷新间隔 + 分段单选）
├─ groupTitle「提醒」+ card.listCard
│  ├─ Toggle × 3
│  ├─ stackRow（阈值滑块，低额度关闭时 data-disabled）
│  └─ settingRowStatic（发送测试通知）
├─ groupTitle「网络代理」+ card.listCard
│  └─ settingRowStatic（代理方式 + 分段单选；指定代理时追加地址输入）
├─ groupTitle「版本」+ card.listCard
│  ├─ settingRowStatic（当前版本 + 更新状态 + 按钮）
│  └─ updateProgress（下载中/已下载时显示）
└─ 若干 groupNote（分组脚注）
```

关键行为：

- 设置页持有本地草稿 `prefs`，用 `dirty` ref 标记未保存改动；连续改动 250ms 防抖后写入 Rust，离开页面前补发一次。
- 版本区块把下载与安装拆成两步：点击“下载更新”后才请求更新包，进度由 `update-download-progress` 事件驱动；下载并通过签名校验后按钮变为“重启并安装”。重新打开设置页时用 `downloaded_update_version` 恢复已下载状态，避免重复下载。
- Windows 免安装 exe 没有可替换的安装目录，不显示“下载更新”，只保留“前往发布页”；macOS 更新替换 `.app` 后重启，Windows 由 NSIS 安装器接管并退出应用。
- 后端状态变化时，只有在没有本地未保存改动时才覆盖草稿。
- 保存成功不打扰用户；只有保存失败时在窗口底部弹一次 `.toast`（4 秒后自动消失，重复失败会重新计时）。
- 主题切换立即应用并单独持久化，不等待防抖保存。
- 开机自启不是普通偏好：先调用系统，成功后再更新界面状态，失败给出明确说明。

### 5.5 菜单栏快速查看（`TrayPanel.tsx`）

菜单栏面板是独立窗口（`tray-panel`，380×560 点），不套用管理窗口的侧栏骨架，只复用颜色 token、格式化函数与 `PlatformLogo`。样式集中在 `src/panels/TrayPanel.module.css`，刻意不放进 `App.module.css`，避免主窗口与面板的布局规则互相牵连。

```
div.panel（透明窗口内的圆角面板）
├─ header
│  ├─ div.brand
│  │  ├─ img.wordmark
│  │  └─ span.updated（账户数 + 更新于 年-月-日 时:分 + 刷新中）
│  └─ button.refreshAll（主色文字「全部刷新」）
├─ div.scroll
│  ├─ p.notice（全局错误，可选）
│  ├─ div.empty（没有任何账户时）
│  ├─ div.sections（Reorder.Group，平台与中转站分组可整组拖动排序）
│  │  └─ section.section × N（Reorder.Item）
│  │     ├─ div.sectionHeadRow
│  │     │  ├─ button.sectionHead（平台徽标 + 名称 + 账户数）
│  │     │  ├─ ReorderHandle（多个平台时）
│  │     │  └─ span.chevron（折叠箭头，点击同样切换）
│  │     └─ div.cardsWrap > div.cards（账户 Reorder.Group）
│  │        └─ article.account × N（Reorder.Item）
│  │           ├─ div.accountHead（账户名 + 重新授权标签 + 相对更新时间 + 刷新 + 排序手柄）
│  │           ├─ span.accountHint（身份，如邮箱；可选）
│  │           ├─ span.tier（套餐档位，主色小标题；可选）
│  │           ├─ div.quotaGroup × N
│  │           │  ├─ span.eyebrow（多套餐时的套餐名，主色小标题；可选）
│  │           │  └─ div.quotaRow（标签 / 额度条 / 「剩余 X%」网格 + 条下说明，说明跨后两列）
│  │           └─ p.errorLine（刷新失败，可选）
│  └─ section.section（中转站，与平台分组同属外层 Reorder.Group；同类标题行 + 排序手柄 + 卡片 Reorder.Group）
└─ footer
   ├─ span.version
   └─ button.openButton「打开主窗口」+ span.divider + button.quitButton「退出」
```

关键行为：

- 订阅账户按平台分组，每个账户一张卡；平台分组可折叠（默认展开），折叠状态只存在组件内存里，不写进设置。
- 面板内也能调整顺序：平台标题右侧、订阅账户与中转站账户的工具区各有与总览相同的排序手柄（共用 `useOverviewOrder` / `useRelayOrder`），拖动或聚焦后按上下方向键移动，写入同一份 `overviewOrder` / `relayOrder`，主窗口与面板随状态事件互相同步；只有一个分组或一个账户时不显示对应手柄。折叠按钮与箭头仍然只负责展开/收起。
- 中转站分组在面板里是与平台分组并列的可拖动项：分组标题右侧也有手柄，位置以标识 `relay` 记入 `overviewOrder.platforms`，可以放到任意平台分组之前或之后；总览页不展示该分组，但拖动平台时会按原槽位保留它。
- 额度行是三列网格：左侧窗口名、中间额度条、右侧「剩余 X%」，重置时间（外加金额/次数）贴在额度条下方并跨到「剩余 X%」那一列，右列下方的空白也用来放说明，长日期不会被省略号截断；真正装不下时才在「·」前换成两行。因此同一张卡里的进度条始终对齐。多套餐账户在每类套餐第一行标注套餐名。
- 账户头右侧显示相对更新时间（`刚刚更新` / `N 分钟前` / 钟点）、单个账户的刷新按钮，以及（同平台有多个账户时）排序手柄；顶部「全部刷新」同时刷新订阅账户与中转站。额度条 6px，颜色与主窗口一致：剩余 ≤70% 且高于提醒阈值转 `--warning` 琥珀色，≤提醒阈值转 `--danger` 红色，未知比例用 `--muted`。
- 中转站账户与订阅账户同卡式排版：账户名 + 相对更新时间 + 刷新按钮，第二行是 `平台 · 主机名`，下面是密钥/账户金额摘要与一条剩余比例条；不复制管理页的两列大卡。
- 页脚不做实色主按钮：左侧版本号，右侧是两个纯文字动作（主色「打开主窗口」、弱化色「退出」），中间用一根竖线分隔。头部「全部刷新」与「打开主窗口」同一套文字动作样式，只换图标与位置。
- 面板打开事件由 Rust 广播（`tray-panel-shown`）；收到后先展示缓存，再按刷新间隔补查中转站。面板只在用户点开时才创建，挂载即视为首次打开，会自己补一次状态同步与过期检查；中转站列表读完前不会主动发请求。
- 面板内按 Esc 收起；点击面板外部由 Rust 的失焦监听收起。主题切换通过 `theme-changed` 事件从主窗口同步，所有窗口统一由 `useThemeSync()` 订阅。
- 文案与数字遵循与主窗口相同的字号/字重级别，但整体缩小一档：面板正文 10–13px，行距更紧，避免在 380px 宽度内换行撑高。时间文案统一走 `src/lib/format.ts` 的 `stampLabel` / `relativeUpdateLabel`。

### 5.6 桌面悬浮球（`FloatingBall.tsx` / `FloatingBallPopover.tsx`）

悬浮球是独立的置顶窗口（`floating-ball`，60×60 点），样式集中在 `src/panels/FloatingBall.module.css`。球体跟随应用外观（浅色 / 深色 / 跟随系统）：浅色下为白底深字，并用更明显的描边与内阴影保证在浅色桌面上仍有边界；深色下保留原来的深蓝黑球体。球体颜色全部来自 `--ball-*` token，弹层沿用 `--surface` / `--border-strong` 等主题 token。主窗口切换外观后通过 `theme-changed` 事件同步到菜单栏面板、悬浮球与弹层。两个窗口都通过 `?view=float` / `?view=float-popover` 复用同一份前端产物。

```
div.stage（60×60；内部边界收起时为 24×60）
└─ div.ball[role=button]（圆形、深色径向渐变）
   ├─ svg.ring（viewBox 0 0 60 60，旋转 -90°；展开时的整圈进度）
   │  ├─ circle.track（轨道）
   │  ├─ circle.progress（剩余比例，按额度等级变主色/琥珀/红/灰）
   │  └─ circle.spinner（刷新中，虚线圆旋转）
   ├─ svg.peekRing（不旋转；只画露出侧的一段弧，进度按这段弧归一化）
   │  ├─ path.track（露出侧的轨道）
   │  ├─ path.progress（归一化后的剩余比例）
   │  └─ circle.spinner（刷新中）
   └─ div.center
      ├─ span.logo（PlatformLogo 12px / 中转站插头图标；收起时移到露出条带正中）
      ├─ span.value（百分比 16px；金额型中转站显示 10px 金额；收起时淡出）
      └─ span.detail（周期：周/月/5小时；或「余额」「选择」「异常」「刷新中」；收起时淡出）

div.popover（340 宽，按内容高度）
├─ 悬停：eyebrow「正在使用」+ 套餐标题 + 套餐档位徽标 + 账户/身份
│  ├─ div.rows > div.row × N（窗口名 / 剩余百分比 / 额度条 / 已用量与重置时间）
│  ├─ p.hoverError（可选）
│  └─ button.footerButton「切换套餐 ›」
└─ 菜单：eyebrow「当前套餐」+ 提示
   └─ div.menuList[role=listbox] > button.item × N
      ├─ span.itemCheck（选中勾）
      ├─ span.planLogo（28px 品牌徽标）
      ├─ span.itemText（套餐名 + 账户/来源）
      └─ span.itemValue（百分比或金额 + 周期）
```

关键行为：

- 球内多周期取最低剩余比例；周期标签取该窗口的短名称（`compactQuotaWindowLabel`，例如「周」「月」「5小时」）。金额型中转站没有可靠比例时不显示百分比，直接在球内显示剩余金额。
- 球体外观使用 `--ball-*` token 跟随主题，额度环使用引用语义色的 `--hud-*`，与主窗口、菜单栏面板共用 `quotaLevel()`：高于 70% 主色，≤70% 且高于阈值琥珀色，≤阈值红色，无可靠比例灰色。
- 球窗口设置为不能成为 key：非 key 窗口的首次点击会被 macOS 用于取 key 而吞掉，不能成为 key 的窗口则让第一次点击直接进入 WebView，同时不抢用户当前应用的键盘焦点。弹层保留 key 能力（菜单需要 Esc 与失焦），并在 `sendEvent:` 收到 mouseDown 时先取 key 再派发，避免详情卡里的「切换套餐」需要点两次。
- 悬停显示详情卡，移出球与卡片后收起；点击球打开套餐切换菜单，再次点击、失焦或按 Esc 收起。弹层打开期间球保持展开。macOS 的悬停事件只派发给 key 窗口，而球是非激活 `NSPanel`，因此“进入”改由 Rust 侧每 150ms 的光标看门狗按窗口矩形补发（不能依赖 `pointerenter`，否则悬停没有反应）；离开仍由前端事件与看门狗宽限期兜底，拖动或菜单结束后要求指针先移出再移入，不会在松手/关闭菜单时立刻重新展开。吸附收起态被悬停时先只展开球，详情卡等 220ms 窗口过渡结束、且指针仍停在完整球体上才显示，避免卡片跟着展开动作一起滑出；自由位置或已经展开的球没有过渡，悬停立即显示。
- 拖动由指针事件判定（移动超过 4 点才开始拖动），靠近左右屏幕边缘吸附并收起成 24 点宽：在虚拟桌面最外侧边缘通过窗口移出屏幕实现；如果该边缘紧接着另一块显示器，则窗口缩到 24×60 点。收起时前端把露出部分重排成 D 形：平台标识移到露出 D 形的视觉中心，百分比与周期淡出，额度环只保留露出侧的那段弧并把进度归一化到这段弧上；两种吸附方式都不会跨屏。悬停展开到完整 60 点（鼠标已贴边时不留 12 点间隙），移出 1 秒后收回。展开按 220ms 带轻微回弹的曲线弹出；收起分两段——先把完整球体滑到边缘（外侧边缘可略微越过落点再弹回，内部边界只用普通缓出），贴边后才切换 D 形布局并收窄窗口，避免球还没贴边就显示成吸附状态。位置与当前套餐写入状态文件，重启恢复。
- 未选择或所选套餐已不存在时显示加号与「选择」，点击打开菜单；查询失败且没有快照时显示「异常」，保留旧快照时不伪装成失败。
- 菜单里订阅套餐与中转站账户并列；中转站标题优先用用户设置的标题，其次主机名，副标题显示提供方与地址。没有可选项时显示空态文案。
- 球窗口只响应鼠标，不进入键盘焦点；弹层按内容高度自适应，宽度固定 340 点。

## 6. 跨页交互约定

### 6.1 刷新

- 刷新是显式动作：图标按钮或带文字按钮，不自动全屏 loading。
- `busy` 由 `product.query.refreshing` 推导；刷新中禁用按钮、图标加 `.spin`。
- 手动刷新失败写入页面错误，不把已有额度清空。
- 浏览器预览下 `desktop === false`，刷新按钮禁用，`title` 说明「浏览器预览无法调用 Rust」。

### 6.2 错误分级

| 级别 | 表现 | 类名 |
| --- | --- | --- |
| 全局 | 状态读取等跨页面错误 | `.noticeDanger.globalNotice`（App 顶部） |
| 瞬时 | 操作成功（导入本地配置）或失败（保存设置、发起授权、添加账户） | `Toast` + `.toast` / `.toastSuccess`（窗口底部居中，自动消失） |
| 页面 | 本页的数据加载/扫描失败 | `.noticeDanger.accountNotice`（RelayPanel）或卡片内 `.noticeDanger` |
| 对象 | 单个套餐/账户刷新失败 | `.noticeStale`（有旧数据）或 `.noticeDanger`（无数据） |
| 行内 | 字段、设置项、套餐状态 | `.settingHint[data-danger]`、`.planRowState[data-state="error"]`、`.quotaLowNote` |

错误文案包含可行动信息；错误码放在 `.noticeAside` 里，不打断主句。

### 6.3 空态

- 整页没有数据：`.overviewEmpty`，给出「为什么空 + 下一步做什么」，不要只写「暂无数据」。
- 卡片里没有数据：`.accountNote`，与卡片字号一致。
- 空态图标使用 Phosphor `duotone` 24px，放在 `.emptyGlyph` 主色底板上。

### 6.4 可访问性清单

新增或改造页面时逐项检查：

- 导航/标签页：`tablist`、`tab`、`tabpanel`，`aria-selected`、`aria-controls`、`aria-labelledby`。
- 折叠区：按钮带 `aria-expanded` 和 `aria-controls`，展开内容有稳定 `id`。
- 进度条：`role="progressbar"` + `aria-valuemin/max/now`，未知时用 `aria-valuetext` 说明。
- 图标按钮：`aria-label`；同时提供 `title` 解释动作，桌面端可悬浮查看。
- 单选药丸组：外层 `role="radiogroup"` + `aria-labelledby`，每个 `input` 有可读标签。
- 开关：`role="switch"` + `aria-labelledby` + `aria-describedby`。
- 弹窗：`role="dialog"`、`aria-modal`、`aria-labelledby`；标题唯一；Esc 关闭、Tab 圈定在弹窗内、关闭后焦点归还（`useDialogA11y`）。
- 焦点：全站 `:focus-visible` 描边；不要移除 outline 后不补替代样式。
- 装饰图标：`aria-hidden`；纯视觉元素不要进入可访问树。

### 6.5 动效与状态切换

- 页面切换、展开收起、弹窗进出必须使用 `AnimatePresence` 或已有动画类，不手写 `setTimeout` 控制显隐。
- 进场用 `ease [0.16, 1, 0.3, 1]`，退场可略慢；减少动态时统一 0。
- 展开动画的父级必须有 `overflow: hidden`（`.quotaBody`），否则高度动画会看到内容溢出。

## 7. 快速改造配方

### 7.1 新增一个侧栏页面

1. 建 `src/panels/FooPanel.tsx`，外层 `<section>`，内容用 `.overviewStack` 或 `.platformStack`。
2. 在 `App.tsx` 的 `PanelKey` 加键。
3. 在 `PANELS` 加一项：`key / label / Icon / title / subtitle`。
4. 在内容区加条件渲染。
5. 如需跨页状态，从 `useAppState()` 取；页面只读，不在页面里重复订阅 Tauri 事件。

```tsx
import styles from '../App.module.css';
import { useAppState } from '../lib/store';

export default function FooPanel() {
  const { state, desktop } = useAppState();

  return (
    <section>
      <div className={styles.overviewStack}>
        {/* 首块不要加 margin-top，壳层已处理 */}
      </div>
    </section>
  );
}
```

### 7.2 新增一个平台

按顺序改：

1. `src/lib/platforms.ts`：在 `PLATFORMS` 补元数据（名称、厂商、单字标记、说明、`available`），在 `PLATFORM_ORDER` 补顺序。
2. `src/components/PlatformLogo.tsx`：有品牌图形时补 `PLATFORM_LOGOS`；没有则自动回退单字标记。
3. `src/panels/AccountsPanel.tsx`：在 `PLATFORM_CHOICES`（入口文案、副标题、两步流程、隐私说明、输入占位、等待态文案）和 `loginCommands` 各补一项。两处都是 `Record<DialogPlatform, ...>`，类型会强制补齐。
4. Rust 侧接入适配器与 IPC；前端不直接请求第三方接口。
5. 补一个「待接入」状态验证：`available: false` 时账户页应显示分组标题 + `plannedNote`，不出现添加入口。

注意：添加位只对登记在 `PLATFORM_CHOICES` 里的平台渲染；平台已在 `platforms.ts` 标记为 `available` 但漏配授权文案时，不会出现添加入口。

### 7.3 复制一个卡片列表页

先选形态：

| 需求 | 复制对象 | 外层 | 单元 |
| --- | --- | --- | --- |
| 同平台多对象，默认折叠 | 总览 | `.platformStack` / `.platformGroup` | `.quotaBoard` + `.quotaAccount` |
| 多对象并排管理 | 账户、中转站账户管理 | `.platformStack` / `.platformGroup` | `.accountGrid` + `.accountTile` |
| 单列、信息密集 | 总览的中转站额度 | `.overviewStack` / `.accountStack` | `.accountCard` |

复用清单：

- 标题：`PlatformHeading` 或 `.groupTitle`。
- 状态：`.authStatus`、`.planTier`、`.planRowState`。
- 错误：`.noticeDanger` / `.noticeStale`。
- 空态：`.overviewEmpty`。
- 刷新：`.refreshButton` 或 `.accountRefresh` + `.spin`。
- 操作：`.miniButton` + `data-danger`。

不要为「新页面」复制一整套 `account*` 样式；优先在现有卡片结构里换内容。

### 7.4 新增一个设置项

| 控件 | 结构 |
| --- | --- |
| 开关 | `Toggle`（`SettingsPanel` 内部组件）→ `.settingRow` + `.switch*` |
| 分段选择 | `.settingRowStatic` + `.pills`（传 `--pill-index` / `--pill-count`） |
| 滑块 | `.stackRow` + `.rangeHead` / `.range` / `.rangeScale`（传 `--range-fill`） |
| 即时动作 | `.settingRowStatic` + `.testButton`（主操作用 `data-primary`） |

流程：

1. 在 `src/lib/desktop.ts` 的 `Preferences` 与 `src/lib/prefs.ts` 的 `DEFAULT_PREFERENCES` 加字段。
2. 在 `SettingsPanel` 的对应分组里加行；改动走 `update(patch)`，由 250ms 防抖统一保存。
3. 需要系统副作用（如自启）时，单独处理 busy / error，不混进普通偏好。
4. 说明文字用 `.settingHint`，分组脚注用 `.settingHint.groupNote`。
5. Rust 侧 `Preferences` 默认值保持同步，否则旧状态文件会缺字段。

### 7.5 新增一个确认弹窗

模板顺序：

1. 在页面的 `DialogState` 联合类型里加一种 `kind`。
2. 在 portal 里的 `AnimatePresence` 中加分支。
3. 结构：`dialogClose` → `dialogHeader`（或裸 `h2`）→ 说明 → `dialogActions`。
4. 危险操作使用 `.dangerButton`，取消使用 `.buttonGhost`。
5. 提交期间用 `acting` 锁按钮，失败保留弹窗并显示错误。
6. 关闭时重置 `acting` 与请求竞态标记。
7. 解构 `useDialogA11y` 返回的 `{ dialogRef, captureTrigger }`；容器挂 `dialogRef` 并设 `tabIndex={-1}`，打开前调用 `captureTrigger(event.currentTarget)`；内容切换时传不同的 `contentKey` 重新落定焦点。

中转站的添加、补充令牌与移除已和账户页统一为弹窗确认；不再维护行内二次确认样式。

### 7.6 抽取共用组件

满足以下条件才抽：

- 至少两个页面使用，且**语义相同**（不是长得像）。
- 抽取后 props 不超过 5 个，不需要页面传入样式。
- 组件放在 `src/components/`，样式仍写进 `src/App.module.css`。
- 纯格式化函数放在 `src/lib/`（例如时间、百分比），不要放在组件文件里各写一份。

## 8. 已知缺口

时间/时长、档位与接入方式文案已收敛到 `src/lib/format.ts`；平台分组统一使用 `.platformGroup`；`.upcoming*` 已删除；弹窗键盘操作由 `useDialogA11y` 提供；平台添加与授权文案由 `PLATFORM_CHOICES` / `loginCommands` 驱动。

| 项 | 现状 | 建议 |
| --- | --- | --- |
| 折叠箭头 | `.chevronButton` 是带 `onClick` 的 `span`，本身不可聚焦 | 保留为纯视觉补充；键盘操作由 `.quotaAccountToggle` 承担，不要给它加 `role="button"` 变成第二个焦点 |

## 9. 类名索引

以下是 `src/App.module.css` 的现有类名分组，新增样式前先在这里找，避免重复造词。菜单栏面板使用独立的 `src/panels/TrayPanel.module.css`，类名以 `panel` / `section` / `account` / `quotaRow` / `relaySummary` 为主，不与管理窗口共用。

| 分区 | 类名 |
| --- | --- |
| 壳层 | `app` `sidebar` `titlebarSafeArea` `wordmark` `nav` `navItem` `navLabel` `navIcon` `navText` `sidebarFoot` `version` `updateChip` `main` `pageHeader` `pageHeaderInner` `panelTitle` `subtitle` `mainScroll` `panelWrap` `footer` |
| 通用卡片/文字 | `card` `listCard` `groupTitle` `usageNote` `settingHint` `groupNote` `codeChip` |
| 平台分组 | `platformStack` `platformGroup` `platformSectionHead` `platformHeading` `platformBadge` `platformLogo` `platformLogoFallback` `platformIdentity` `platformNameRow` `platformName` `platformVendor` `platformDescription` `platformAside` `platformCount` `platformStatus` `plannedNote` |
| 总览额度板 | `overviewTabs` `overviewTab` `overviewStack` `overviewEmpty` `emptyGlyph` `emptyTitle` `quotaBoard` `quotaAccount` `quotaAccountHead` `quotaAccountToggle` `quotaAccountInfo` `quotaAccountIdentity` `quotaAccountName` `quotaSummary` `quotaAccountTools` `chevronButton` `chevronIcon` `quotaAccountMeta` `metaDot` `quotaBody` `quotaBodyInner` `quotaNotices` `metricGrid` `metric` `metricDivider` `metricEyebrow` `metricProduct` `metricLabel` `quotaBarRow` `quotaPercent` `quotaBarTrack` `quotaBarFill` `metricMeta` `metricMetaPart` `metricDot` `quotaMeta` `quotaLowNote` `dataGaps` `skel` `skelLabel` `skelValue` `skelBar` `skelMeta` |
| 账户管理 | `accountGrid` `accountTile` `accountIdentityCell` `accountMark` `accountIdentity` `accountNameRow` `accountName` `accountMeta` `planPanel` `planRow` `planRowMain` `planRowName` `planTier` `planRowState` `accountFoot` `accountRefresh` `accountActions` `miniButton` `accountAddTile` `accountAddIcon` `accountAddLabel` `accountEmptyNote` `accountNotice` |
| 中转站 | `relayQuotaView` `relayScanButton` `relayTileStatus` `relayTileMeta` `relayTileNotice` `relaySectionLabel` `relayAmount` `relayQuotaGrid` `relayQuotaCell` `relayQuotaLabel` `relayQuotaCaption` `relayMeterRow` `relayMeterValue` `relayBar` `relayBarFill` `relayWindows` `relayWindow` `relayWindowHead` `relaySkipped` `relaySkippedToggle` `accountStack` `accountCard` `accountHead` `accountTitleBlock` `accountMetaLine` `accountHint` `accountTools` `updatedAt` `refreshButton` `chevronButton` `chevronIcon` |
| 状态/提示 | `noticeDanger` `noticeStale` `noticeAside` `globalNotice` `authStatus` `authDot` `toast` `toastSuccess` `planTag` |
| 设置 | `settingRow` `settingRowStatic` `settingText` `settingLabel` `stackRow` `pills` `pill` `pillThumb` `pillText` `switchWrap` `switchInput` `switchTrack` `switchKnob` `range` `rangeHead` `rangeValue` `rangeScale` `testButton` `buttonRow` |
| 表单/按钮 | `field` `fieldLabel` `textInput` `button` `buttonGhost` `dangerButton` |
| 弹窗 | `dialogBackdrop` `dialog` `dialogClose` `dialogHeader` `dialogSubtitle` `dialogSteps` `dialogStep` `stepMarker` `stepBody` `stepTitle` `stepText` `dialogFieldGroup` `dialogNote` `authBlock` `authUrl` `loginStatus` `dialogSuccess` `dialogSuccessIcon` `dialogSuccessBody` `dialogSuccessTitle` `dialogSuccessText` `dialogActions` `dialogForm` |
| 动效 | `spin` |

## 10. 维护约定

1. **样式只有一个入口**：新类写进 `src/App.module.css` 的对应分区，并按 `/* ---------- 分区名 ---------- */` 注释归类。
2. **状态用 `data-*`**：`data-level`（额度等级）、`data-stacked`、`data-state`、`data-active`、`data-danger`、`data-disabled`，不要为每个状态造一个类。
3. **命名按语义**：用 `quotaAccount`、`planRowState` 这类描述角色的名字，不用 `box1`、`rowLeft`。
4. **组件归属**：跨页面复用放 `src/components/`；单页辅助组件和格式化函数留在页面文件内，直到出现第二个使用者。
5. **文案中文、术语统一**：额度、套餐、账户、授权、刷新、重新授权、移除；同一动作在不同页面用同一个词。
6. **改动同步文档**：页面结构、组件职责或复用方式变化时更新本文；业务规则变化更新需求/软件设计文档。
7. **不加第三方 UI 库**：现有交互（开关、药丸、弹窗、骨架）都用原生元素 + CSS 实现，保持依赖面和视觉一致。