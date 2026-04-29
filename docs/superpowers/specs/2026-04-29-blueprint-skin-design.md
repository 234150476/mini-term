# Blueprint 皮肤设计

**日期**: 2026-04-29
**状态**: 已确认

## 概述

为 mini-term 新增 Blueprint（蓝图）主题皮肤，源自工程制图视觉语言。皮肤作为独立于 dark/light/auto 主题之上的视觉叠加层，通过 `data-skin` 属性控制激活。

## 设计决策

| 决策项 | 选择 | 原因 |
|--------|------|------|
| 集成方式 | 皮肤系统（与主题正交） | 不破坏现有主题逻辑，可扩展更多皮肤 |
| 视觉深度 | B 级（全局蓝图化） | 所有面板统一风格，但不加重装饰 |
| 字体策略 | UI 文字等宽化，终端不变 | 终端字体用户自选，不应被覆盖 |
| 实现方式 | 纯 CSS 变量覆盖 + data-skin | 零组件侵入，样式层解决 |

## 色彩体系

```
背景基色:     #0a1628 (深普鲁士蓝)
表面色:       #0f1f38
抬升色:       #162a4a
叠加色:       #1a365d
终端背景:     #060e1c

主文字:       rgba(255,255,255,0.85)
次文字:       rgba(255,255,255,0.6)
弱文字:       rgba(96,165,250,0.5)

主边框:       rgba(96,165,250,0.25)
默认边框:     rgba(96,165,250,0.2)
弱边框:       rgba(96,165,250,0.12)

强调色:       #22d3ee (青色)
辅助色:       #60a5fa (浅蓝)
成功色:       #22c55e
警告色:       #f97316
错误色:       #ef4444
AI 色:        #a78bfa
```

## 网格系统

使用 CSS `background-image` 叠加两层网格，挂在全局伪元素或 body 上：

- 次网格: 20px 间距，`rgba(255,255,255,0.03)` 白色细线
- 主网格: 100px 间距，`rgba(96,165,250,0.08)` 浅蓝粗线
- `pointer-events: none` 确保不影响交互

## 面板视觉元素

### 角标记

每个面板（panel）使用 `::before` 和 `::after` 伪元素绘制 L 型角标记：

```css
.panel::before {
  top: 0; left: 0;
  border-top: 1.5px solid rgba(34,211,238,0.6);
  border-left: 1.5px solid rgba(34,211,238,0.6);
  width: 6px; height: 6px;
}
.panel::after {
  bottom: 0; right: 0;
  border-bottom: 1.5px solid rgba(34,211,238,0.6);
  border-right: 1.5px solid rgba(34,211,238,0.6);
  width: 6px; height: 6px;
}
```

### 面板标题

- 大写字母 + letter-spacing: 1.5px
- 等宽字体
- 颜色: `rgba(96,165,250,0.7)`

### Tab 标签

- 大写编号: "TERMINAL 01"
- 活动 Tab 底部青色指示线
- 状态点保持现有颜色语义

## 交互效果

| 触发 | 效果 |
|------|------|
| 元素悬停 | 边框变亮 + 青色 box-shadow 光晕 (0 0 10-15px) |
| 角标记悬停 | L 型标记扩大 (6px → 10px) |
| 文件项悬停 | 左侧出现青色竖线指示 |
| 面板悬停 | 微弱内发光 inset box-shadow |
| 按钮悬停 | 边框变青 + 外发光 |

过渡时间统一 0.2-0.25s ease。

## 字体

Blueprint 皮肤激活时，UI 区域字体栈切换为：

```css
font-family: 'Courier New', Consolas, 'Liberation Mono', monospace;
```

不加载外部字体。终端区域字体保持用户配置不变。

## 终端配色 (xterm.js)

当 `terminalFollowTheme: true` 且 skin 为 blueprint 时使用：

```typescript
BLUEPRINT_TERMINAL_THEME = {
  background: '#060e1c',
  foreground: '#d9e2ec',
  cursor: '#22d3ee',
  cursorAccent: '#060e1c',
  selectionBackground: 'rgba(34,211,238,0.2)',
  black: '#0a1628',
  red: '#ef4444',
  green: '#22c55e',
  yellow: '#f97316',
  blue: '#60a5fa',
  magenta: '#a78bfa',
  cyan: '#22d3ee',
  white: '#e2e8f0',
  brightBlack: '#1a365d',
  brightRed: '#f87171',
  brightGreen: '#4ade80',
  brightYellow: '#fb923c',
  brightBlue: '#93c5fd',
  brightMagenta: '#c4b5fd',
  brightCyan: '#67e8f9',
  brightWhite: '#f8fafc',
}
```

## 架构: 文件变更清单

| 文件 | 变更类型 | 说明 |
|------|----------|------|
| `src/types.ts` | 修改 | `AppConfig.skin: 'none' \| 'blueprint'` |
| `src/store.ts` | 修改 | 默认值 + `applySkin()` |
| `src/styles.css` | 修改 | `[data-skin="blueprint"]` 变量覆盖块 |
| `src/blueprint.css` | 新增 | 网格、角标记、光晕、字体等专属样式 |
| `src/App.tsx` | 修改 | useEffect 监听 skin 变化设置 data-skin |
| `src/components/SettingsModal.tsx` | 修改 | 皮肤选择器 UI |
| `src/utils/terminalCache.ts` | 修改 | BLUEPRINT_TERMINAL_THEME + 选择逻辑 |
| `src-tauri/src/config.rs` | 修改 | AppConfig 加 skin 字段 |

## Settings UI

在"系统设置"页面，主题按钮组下方新增皮肤按钮组：

```
[主题]  ○ 深色  ○ 浅色  ○ 自动
[皮肤]  ○ 无    ○ 蓝图
```

皮肤切换立即生效，无需重启。切换时调用 `applySkin()` 更新 `data-skin` 属性并同步终端配色。

## 滚动条

Blueprint 皮肤下自定义 webkit scrollbar:

```css
::-webkit-scrollbar { width: 4px; }
::-webkit-scrollbar-track { background: transparent; }
::-webkit-scrollbar-thumb { background: rgba(96,165,250,0.2); }
::-webkit-scrollbar-thumb:hover { background: rgba(96,165,250,0.4); }
```
