# Stele — Product Requirements Document



## 1. 项目定位

Stele 是一个像素级排版的现代终端模拟器。它是一个**真正的终端**——PTY、shell、VT 协议完整支持——但渲染层抛弃了传统的等宽 cell 网格，使用 GPU 驱动的像素级文本排版和矢量图形绘制。

```
传统终端：  VT 协议 → cell 网格 → 等宽字体 → CPU 渲染
Stele：    VT 协议 → Cell Adapter → 像素坐标 → 比例字体 + 矢量图元 → GPU 渲染
```

## 2. 核心问题

终端的 cell 网格模型已经 50 年没变过。它带来两个根本限制：

1. **文本排版粗糙**：等宽字体是唯一选择，字间距、行间距不可调，无法使用比例字体
2. **图形能力缺失**：Sixel/Kitty graphics 只能贴光栅图，无法在终端中做矢量绘图

这两个限制的根源是同一个：**终端的最小寻址单位是 cell，不是像素。**

Stele 把最小寻址单位从 cell 降到像素。

## 3. 用户画像

- 日常使用终端的开发者，希望终端更美观、排版更精细
- 需要在终端中展示图表、绘图的 CLI 工具开发者
- 对终端技术本身感兴趣的极客

## 4. 核心需求

### 4.1 完整的终端模拟器

**必须**是一个真正的终端：

- 启动后呈现 `zhanwei@zhanwei:~$`，可以运行 shell、ssh、git 等一切终端程序
- PTY 管理（fork/exec、信号转发、窗口大小通知）
- VT100/VT220/xterm 转义序列解析（文本属性、光标移动、滚动、alternate screen 等）
- 剪贴板、选择、滚动回看等基本终端交互

### 4.2 像素级文本排版（pretext 模型）

借鉴 chenglou/pretext 的两阶段架构：**prepare（测量）和 layout（排版）分离**。

```
阶段一：prepare（低频，字体/内容变化时触发）
  screen buffer 内容 + 字体参数
    → 字体引擎测量每个 segment 的像素宽度
    → 缓存到 PreparedScreen（segment → width 的 Map）

阶段二：layout（高频，每帧可调用）
  PreparedScreen + viewport 宽度
    → 纯算术：累加宽度、计算像素坐标、对齐修正
    → 输出 LayoutResult：每个字符的 (x, y) 像素坐标
    → 零字体引擎调用，零内存分配
```

这个分离带来的关键能力：
- **字体动态变化**：字体参数每帧变化时，只需重新 prepare 变化的 segment，layout 依然是纯算术
- **resize 零延迟**：窗口大小变化只触发 layout（不需要重新测量），因为宽度数据已缓存
- **对齐修正可以在 layout 阶段做**：基于缓存的宽度数据做空格弹性调整，不涉及字体引擎

要求：
- 支持比例字体（非等宽）
- segment 级测量缓存：相同 (segment, font) 只测量一次
- CJK 字符按 grapheme 逐个测量（参考 pretext 的 CJK 处理）
- 禁则处理：CJK 行首/行末标点规则（kinsoku）
- 行高基于字体 metrics（ascent + descent + line gap），非固定 cell 高度
- layout 热路径无内存分配、无字体引擎调用

### 4.3 Cell Adapter

这是 Stele 的核心抽象——在 VT cell 协议和像素渲染之间的桥梁。内部使用 pretext 模型。

```
VT 协议: "光标到 (row=3, col=10)"
    ↓
Cell Adapter:
  1. 查 PreparedScreen 缓存，取第 3 行前 10 个字符的测量宽度
  2. 纯算术累加 → 得到像素 x 坐标
  3. 对齐修正：检测空格序列，微调间距
    ↓
像素坐标: (x=87.5, y=54.0)
```

职责：
- 维护 screen buffer，每个逻辑 cell 记录字符内容和属性
- 持有 PreparedScreen 缓存，screen buffer 变化时增量更新（只 prepare 脏行）
- `(row, col) → (x, y)` 坐标映射：基于缓存的纯算术计算
- 对齐修正算法：检测空格序列，微调间距使视觉对齐到逻辑 cell 边界
- 当终端 resize 时只触发 layout（不重新测量）
- 为 alternate screen 模式的应用（vim、htop）提供足够准确的映射

### 4.4 矢量图形协议

自定义转义序列，支持在终端中绘制矢量图元：

```
\e_G line;x1;y1;x2;y2;color \e\\           — 画直线
\e_G rect;x;y;w;h;fill;stroke \e\\         — 画矩形
\e_G circle;cx;cy;r;fill;stroke \e\\       — 画圆
\e_G curve;cx;cy;a;b;type;color \e\\       — 画曲线（椭圆/双曲线/抛物线）
\e_G path;d \e\\                            — SVG path 语法子集
\e_G text;x;y;font;size;color;content \e\\ — 像素定位文字
\e_G clear \e\\                             — 清除图形层
\e_G layer;create;id \e\\                   — 图层管理
\e_G layer;destroy;id \e\\
\e_G layer;z;id;order \e\\
```

坐标系：原点 `(0,0)` 在视口左上角，单位为逻辑像素。

### 4.5 GPU 渲染

- 使用 wgpu 做 GPU 加速渲染
- 文本光栅化（cosmic-text / swash / fontdue，待选型）
- 矢量图元绘制（lyon / 自研 tessellation，待选型）
- 文本层 + 图形层合成
- 支持 HiDPI / Retina

## 5. 架构概要

```
┌─────────────────────────────────────┐
│           Shell / 应用               │
│   VT 转义序列 + 图形转义序列         │
└──────────────┬──────────────────────┘
               │ PTY
┌──────────────▼──────────────────────┐
│           协议解析层                  │
│  ┌──────────┐  ┌──────────────────┐ │
│  │ VT Parser │  │ Graphics Parser  │ │
│  └─────┬────┘  └────────┬─────────┘ │
└────────┼────────────────┼───────────┘
         │                │
┌────────▼────────────────▼───────────┐
│           Scene                      │
│  ┌────────────┐  ┌───────────────┐  │
│  │ Cell Buffer │  │ Graphics Layer│  │
│  │ (文本+光标) │  │ (图元列表)    │  │
│  └─────┬──────┘  └──────┬────────┘  │
└────────┼────────────────┼───────────┘
         │                │
┌────────▼────────────────▼───────────┐
│  Cell Adapter (pretext 模型)         │
│  ┌─────────────┐  ┌──────────────┐  │
│  │ prepare()   │  │ layout()     │  │
│  │ 字体测量    │  │ 纯算术定位   │  │
│  │ segment缓存 │  │ 对齐修正     │  │
│  └─────────────┘  └──────────────┘  │
│  cell (row,col) → pixel (x,y)       │
└────────────────┬────────────────────┘
                 │
┌────────────────▼────────────────────┐
│        GPU Renderer (wgpu)          │
│  文本光栅化 + 图元绘制 + 合成        │
└─────────────────────────────────────┘
```

### 数据流（pretext 模型）

```
内容变化（字符写入/删除）     字体变化（用户切换字体/动画）
        │                              │
        ▼                              ▼
  增量 prepare()                 全量 prepare()
  (只测量脏 segment)            (重新测量所有 segment)
        │                              │
        └──────────┬───────────────────┘
                   ▼
            PreparedScreen
         (segment → width 缓存)
                   │
    ┌──────────────┼──────────────┐
    │              │              │
    ▼              ▼              ▼
 layout()      layout()       layout()
 (当前宽度)   (resize 后)   (每帧/动画)
    │              │              │
    ▼              ▼              ▼
 LayoutResult: 每个字符的 (x, y, w, h)
                   │
                   ▼
              GPU 渲染
```

## 6. 非目标（v1 不做）

- 不做 tab / split pane 等窗口管理（先做好单窗口）
- 不做 Sixel / Kitty graphics 兼容（先实现自己的协议）
- 不做终端复用（tmux 功能）
- 不做配置 GUI（配置文件即可）
- 不做跨平台（先 Linux，后续考虑 macOS）

## 7. 技术选型（初步）

| 领域 | 候选 | 备注 |
|---|---|---|
| 语言 | Rust | 主力语言 |
| GPU | wgpu | 跨后端（Vulkan/Metal/DX12） |
| 文本光栅化 | cosmic-text / swash | 需要评估 |
| 矢量图元 | lyon / 自研 | 需要评估 |
| VT 解析 | vte crate | 成熟，xterm 兼容 |
| PTY | rustix / nix | POSIX PTY 操作 |
| 窗口 | winit | 窗口创建 + 事件循环 |
| 字体 | fontdb + swash | 字体发现 + 光栅化 |

## 8. 开放问题

1. **对齐修正算法的具体策略**：空格弹性伸缩的上下限是什么？过度拉伸会不会看起来奇怪？需要原型验证
2. **alternate screen 下的体验**：vim/htop 等全屏应用在比例字体下的表现，是否需要 fallback 到等宽模式？
3. **文本选择模型**：比例字体下的选择区域计算，双击选词的边界判定
4. **性能目标**：输入延迟、帧率、大量文本输出时的吞吐量，需要定义基准
5. **图形协议的坐标空间**：逻辑像素 vs 物理像素 vs 相对于视口的百分比？
6. **字体 fallback 链**：比例字体 → CJK fallback → emoji fallback → 等宽 fallback 的优先级

## 9. 里程碑（草案）

| 阶段 | 目标 | 交付物 |
|---|---|---|
| M0 | 窗口 + GPU 文本渲染 | 能在窗口中用比例字体渲染一段文本 |
| M1 | PTY + VT 基础 | 能启动 shell，输入命令，看到输出 |
| M2 | Cell Adapter | 比例字体下 VT 坐标正确映射，对齐修正 |
| M3 | 基本可用 | 能跑日常 shell 工作流，滚动、选择、剪贴板 |
| M4 | 图形协议 v1 | 支持基本图元绘制（线、圆、矩形、路径） |
| M5 | 图层 + 合成 | 文本层与图形层独立管理和合成 |
