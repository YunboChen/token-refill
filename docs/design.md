# Token Refill 界面设计规范

- 文档版本：0.1
- 最近更新：2026-10-24
- 适用范围：管理窗口前端（React + TypeScript）
- 事实来源：`src/App.module.css`、`src/styles.css`、`src/App.tsx`、`src/panels/`、`src/components/`、`src/lib/format.ts`、`src/lib/useDialogA11y.ts`
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
4. **主色克制**：强调色只用于选中态、主按钮、可点击的强调信息；异常色（`--danger`）只出现在失败、低额度、需要重新授权。正常状态保持中性。
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
| `--danger` / `--danger-soft` | `#b4233b` / `#fbeaed` | `#ff8b9c` / `#341c23` | 失败、低额度、破坏性操作 |
| `--focus` | `#0b63ce` | `#79b8ff` | 键盘焦点描边 |
| `--accent-ring` | 14% 主色 | 18% 主色 | 输入框聚焦光晕 |

使用规则：

- **文字强调用 `--accent`，实色块用 `--accent-strong`**，不要反过来。
- 新颜色先问是否能用现有语义 token；确需新增时同时补浅色、深色和系统跟随三处。
- 不在组件里写十六进制色值（弹窗遮罩、开关滑块等高光/中性黑除外）。

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

- macOS 使用 Overlay 标题栏：`.titlebarSafeArea` 高 44px，承载红绿灯并作为拖拽区。
- Windows 使用系统标题栏：`data-platform="windows"` 时安全区降为 10px。
- 页头、标题、副标题均带 `data-tauri-drag-region`；按钮和可交互元素不要加，否则会吞掉点击。
- 平台信息来自 `getAppInfo()`，读取失败保持 macOS 布局，不阻塞界面。

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
| `.accountStack` | 单列卡片列表 | `gap: 10px`；中转站使用 |
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
| `.planTag` | 中性标签 | 10.5px 胶囊，中转站来源/额度模式使用 |
| `.providerTag` | 强调标签 | 主色胶囊，中转站平台名使用 |
| `.draftTag` | 描边标签 | 设置页「本机」标签 |

### 4.3 账户与额度展示

现有三种账户卡形态，**新页面按信息密度选一种，不要混用**：

| 形态 | 类名 | 适合场景 | 参考页面 |
| --- | --- | --- | --- |
| 可折叠额度板 | `.quotaBoard` + `.quotaAccount*` + `.metric*` | 同平台多账户，默认只看摘要，展开看完整窗口 | 总览 |
| 管理网格卡 | `.accountGrid` + `.accountTile*` | 需要并排比较、每张卡都有独立操作 | 账户 |
| 单列详情卡 | `.accountStack` + `.accountCard*` | 每个对象信息较多、按时间或来源排列 | 中转站 |

总览额度板关键结构：

| 类名 | 作用 |
| --- | --- |
| `.quotaAccount` | 单个账户；`data-open` 控制展开态的内边距 |
| `.quotaAccountHead` / `.quotaAccountToggle` | 整行可点的展开按钮，含名称、档位、折叠摘要 |
| `.quotaAccountIdentity` / `.quotaAccountName` / `.quotaSummary` | 名称与摘要共用一条基线；摘要单行省略 |
| `.quotaAccountTools` / `.chevronButton` / `.chevronIcon` | 更新时间、刷新按钮、箭头；箭头只是视觉补充，键盘操作由左侧按钮承担 |
| `.quotaBody` / `.quotaBodyInner` | 折叠动画的裁剪层与内层留白 |
| `.quotaAccountMeta` / `.metaDot` | 展开后的身份说明与分隔点 |
| `.metricGrid` / `.metric` | 横向排布的额度窗口；条目少时平分宽度 |
| `.metricDivider` | 窗口间竖线；`data-strong` 表示套餐分界，竖线占满整行高度 |
| `.metricEyebrow` / `.metricProduct` | 多套餐时的眉线，固定高度保证下方标签对齐 |
| `.metricLabel` / `.metricMeta` / `.metricMetaPart` / `.metricDot` | 窗口名、已用/重置说明；`data-stacked` 时分成两行；窗口宽度 ≤1050px 时隐藏 `.metricDot` |
| `.quotaBarRow` / `.quotaPercent` / `.quotaBarTrack` / `.quotaBarFill` | 百分比 + 进度条；`data-low` 转危险色，`data-unknown` 弱化 |
| `.quotaLowNote` | 低于提醒阈值的红色说明 |
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

中转站单列卡关键结构：

| 类名 | 作用 |
| --- | --- |
| `.accountCard` / `.accountHead` / `.accountTitleBlock` | 卡片与标题区 |
| `.accountMetaLine` / `.accountHint` | 来源、平台、额度模式、密钥名等标签行 |
| `.accountTools` / `.refreshButton` / `.updatedAt` | 右侧刷新与更新时间 |
| `.relayForm` | 地址 + 密钥 + 按钮的横向表单 |
| `.relaySummary` / `.relayAmount` | 金额摘要区 |
| `.relayBarWrap` / `.relayBar` / `.relayBarFill` / `.relayScale` | 剩余比例条；`data-low` 转危险色 |
| `.relayMetaLine` / `.relayWindows` / `.relayWindow` / `.relayWindowHead` | 到期时间与分窗口额度 |
| `.relaySkipped` | 未识别的本地配置列表 |

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
| `.saveState` | 设置保存状态 | `data-state="error"` 转红；跟随 `.draftBanner` |
| `.draftBanner` / `.draftIcon` / `.draftText` | 中性信息条 | 设置页顶部 |
| `.inlineConfirm` / `.inlineConfirmActions` | 行内二次确认 | 中转站移除；比弹窗更轻 |
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

敏感输入（如中转站 API Key）应使用**非受控 ref**读取，不进入 React 状态、不写入日志或状态文件。现有 `RelayPanel` 是范例。

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
| `.buttonRow` | 按钮并排 | 设置页版本检查使用 |

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
└─ overviewStack
   ├─ 空态（无账户）overviewEmpty
   └─ 每个平台 platformGroup
      ├─ platformSectionHead → PlatformHeading
      └─ quotaBoard
         ├─ quotaAccount（展开：第一个账户）
         │  ├─ quotaAccountHead（名称/档位/摘要 + 刷新 + 箭头）
         │  └─ quotaBody > quotaBodyInner
         │     ├─ quotaAccountMeta（身份 · 来源 · 授权状态）
         │     ├─ 提示（stale / danger）
         │     ├─ metricGrid（窗口 / 分隔线）
         │     └─ dataGaps
         └─ quotaAccount（折叠：其余账户，一行摘要）
```

关键行为：

- 平台顺序由 `groupAccountsByPlatform()` 固定，账户增删不会让分组跳动。
- 同一平台的账户共用一张卡；默认展开第一个，`undefined` 表示沿用默认，`null` 表示全部收起。
- 折叠摘要由 `accountSummary()` 生成：单套餐列各窗口余量，多套餐按「套餐 · 窗口」逐段列出。
- `buildMetrics()` 把「套餐 → 额度组 → 窗口」拍平成格子；多套餐时显示眉线，单类套餐内有多个额度组时显示组名。
- 展开体只渲染在 `open` 时，折叠时 DOM 不保留内容。

### 5.2 账户（`AccountsPanel.tsx`）

```
section
├─ loginError 提示（本页专属）
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

### 5.3 中转站（`RelayPanel.tsx`）

```
section → overviewStack
├─ card（识别表单）
│  ├─ relayForm（地址 / API Key / 识别并查询 / 重新扫描）
│  ├─ usageNote（读取哪些本地配置、密钥只留在内存）
│  ├─ relaySkipped（未识别的配置）
│  ├─ 浏览器预览说明
│  └─ formError
├─ 空态 overviewEmpty
└─ accountStack → accountCard × N
   ├─ accountHead（host + 标签 + 工具）
   ├─ inlineConfirm（移除确认）
   ├─ 上次刷新失败提示
   └─ relaySummary / relayMetaLine / relayWindows
```

关键行为：

- 进入页面自动扫描一次本地配置；「重新扫描」手动重试。
- API Key 用 ref 读取，不进入组件状态。
- 移除使用行内确认而不是弹窗，减少打断。
- 金额、比例、到期时间、窗口额度都做了「未知/不限额」的分支，不把未知显示成 0。

### 5.4 设置（`SettingsPanel.tsx`）

```
section
├─ draftBanner（本机保存说明 + 保存状态）
├─ groupTitle「刷新」+ card.listCard
│  └─ settingRowStatic（自动刷新间隔 + 分段单选）
├─ groupTitle「提醒」+ card.listCard
│  ├─ Toggle × 3
│  ├─ stackRow（阈值滑块，低额度关闭时 data-disabled）
│  └─ settingRowStatic（发送测试通知）
├─ groupTitle「系统」+ card.listCard
│  ├─ settingRowStatic（外观 + 分段单选）
│  └─ Toggle（开机自启）
├─ groupTitle「版本」+ card.listCard
│  └─ settingRowStatic（当前版本 + 更新状态 + 按钮）
└─ 若干 groupNote（分组脚注）
```

关键行为：

- 设置页持有本地草稿 `prefs`，用 `dirty` ref 标记未保存改动；连续改动 250ms 防抖后写入 Rust，离开页面前补发一次。
- 后端状态变化时，只有在没有本地未保存改动时才覆盖草稿。
- 保存状态显示在 `.draftBanner` 右侧（保存中 / 已保存 / 保存失败）。
- 主题切换立即应用并单独持久化，不等待防抖保存。
- 开机自启不是普通偏好：先调用系统，成功后再更新界面状态，失败给出明确说明。

## 6. 跨页交互约定

### 6.1 刷新

- 刷新是显式动作：图标按钮或带文字按钮，不自动全屏 loading。
- `busy` 由 `product.query.refreshing` 推导；刷新中禁用按钮、图标加 `.spin`。
- 手动刷新失败写入页面错误，不把已有额度清空。
- 浏览器预览下 `desktop === false`，刷新按钮禁用，`title` 说明「浏览器预览无法调用 Rust」。

### 6.2 错误分级

| 级别 | 表现 | 类名 |
| --- | --- | --- |
| 全局 | 状态读取、授权入口等跨页面错误 | `.noticeDanger.globalNotice`（App 顶部） |
| 页面 | 添加授权失败、表单错误 | `.noticeDanger.accountNotice` 或卡片内 `.noticeDanger` |
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
| 多对象并排管理 | 账户 | `.platformStack` / `.platformGroup` | `.accountGrid` + `.accountTile` |
| 单列、信息密集 | 中转站 | `.overviewStack` / `.accountStack` | `.accountCard` |

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

比弹窗更轻的破坏性操作优先用 `.inlineConfirm`（参考中转站移除）。

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

以下是 `src/App.module.css` 的现有类名分组，新增样式前先在这里找，避免重复造词。

| 分区 | 类名 |
| --- | --- |
| 壳层 | `app` `sidebar` `titlebarSafeArea` `wordmark` `nav` `navItem` `navLabel` `navIcon` `navText` `sidebarFoot` `version` `updateChip` `main` `pageHeader` `pageHeaderInner` `panelTitle` `subtitle` `mainScroll` `panelWrap` `footer` |
| 通用卡片/文字 | `card` `listCard` `groupTitle` `usageNote` `settingHint` `groupNote` `codeChip` |
| 平台分组 | `platformStack` `platformGroup` `platformSectionHead` `platformHeading` `platformBadge` `platformLogo` `platformLogoFallback` `platformIdentity` `platformNameRow` `platformName` `platformVendor` `platformDescription` `platformAside` `platformCount` `platformStatus` `plannedNote` |
| 总览额度板 | `overviewStack` `overviewEmpty` `emptyGlyph` `emptyTitle` `quotaBoard` `quotaAccount` `quotaAccountHead` `quotaAccountToggle` `quotaAccountIdentity` `quotaAccountName` `quotaSummary` `quotaAccountTools` `chevronButton` `chevronIcon` `quotaAccountMeta` `metaDot` `quotaBody` `quotaBodyInner` `quotaNotices` `metricGrid` `metric` `metricDivider` `metricEyebrow` `metricProduct` `metricLabel` `quotaBarRow` `quotaPercent` `quotaBarTrack` `quotaBarFill` `metricMeta` `metricMetaPart` `metricDot` `quotaMeta` `quotaLowNote` `dataGaps` `skel` `skelLabel` `skelValue` `skelBar` `skelMeta` |
| 账户管理 | `accountGrid` `accountTile` `accountIdentityCell` `accountMark` `accountIdentity` `accountNameRow` `accountName` `accountMeta` `planPanel` `planRow` `planRowMain` `planRowName` `planTier` `planRowState` `accountFoot` `accountRefresh` `accountActions` `miniButton` `accountAddTile` `accountAddIcon` `accountAddLabel` `accountEmptyNote` `accountNotice` |
| 中转站 | `relayForm` `relaySummary` `relayAmount` `relayBarWrap` `relayBar` `relayBarFill` `relayScale` `relayMetaLine` `relayWindows` `relayWindow` `relayWindowHead` `relaySkipped` `accountStack` `accountCard` `accountHead` `accountTitleBlock` `accountMetaLine` `accountHint` `accountTools` `updatedAt` `refreshButton` `inlineConfirm` `inlineConfirmActions` |
| 状态/提示 | `noticeDanger` `noticeStale` `noticeAside` `globalNotice` `authStatus` `authDot` `draftBanner` `draftIcon` `draftTag` `draftText` `saveState` `planTag` `providerTag` |
| 设置 | `settingRow` `settingRowStatic` `settingText` `settingLabel` `stackRow` `pills` `pill` `pillThumb` `pillText` `switchWrap` `switchInput` `switchTrack` `switchKnob` `range` `rangeHead` `rangeValue` `rangeScale` `testButton` `buttonRow` |
| 表单/按钮 | `field` `fieldLabel` `textInput` `button` `buttonGhost` `dangerButton` |
| 弹窗 | `dialogBackdrop` `dialog` `dialogClose` `dialogHeader` `dialogSubtitle` `dialogSteps` `dialogStep` `stepMarker` `stepBody` `stepTitle` `stepText` `dialogFieldGroup` `dialogNote` `authBlock` `authUrl` `loginStatus` `dialogSuccess` `dialogSuccessIcon` `dialogSuccessBody` `dialogSuccessTitle` `dialogSuccessText` `dialogActions` `dialogForm` |
| 动效 | `spin` |

## 10. 维护约定

1. **样式只有一个入口**：新类写进 `src/App.module.css` 的对应分区，并按 `/* ---------- 分区名 ---------- */` 注释归类。
2. **状态用 `data-*`**：`data-open`、`data-low`、`data-unknown`、`data-stacked`、`data-state`、`data-active`、`data-danger`、`data-disabled`，不要为每个状态造一个类。
3. **命名按语义**：用 `quotaAccount`、`planRowState` 这类描述角色的名字，不用 `box1`、`rowLeft`。
4. **组件归属**：跨页面复用放 `src/components/`；单页辅助组件和格式化函数留在页面文件内，直到出现第二个使用者。
5. **文案中文、术语统一**：额度、套餐、账户、授权、刷新、重新授权、移除；同一动作在不同页面用同一个词。
6. **改动同步文档**：页面结构、组件职责或复用方式变化时更新本文；业务规则变化更新需求/软件设计文档。
7. **不加第三方 UI 库**：现有交互（开关、药丸、弹窗、骨架）都用原生元素 + CSS 实现，保持依赖面和视觉一致。