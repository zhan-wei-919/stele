# Stele 架构文档

## 一句话描述

Stele 是一个终端模拟器。当前阶段（M0/M1）先采用类似浏览器 `<pre>` 的原生文本流模型：PTY 输出按行存储，逐行测量并直接渲染到像素坐标。M2 再引入 Cell Adapter，把 VT 的 `row/col` 语义映射到同一套渲染层，为 `vim`、`top`、`nvim` 等强依赖网格语义的程序提供兼容路径。

---

## 设计原则

1. **先做最短闭环**：先跑通 `shell -> 文本行 -> GPU`，不要一开始就做完整 cell 兼容层。
2. **渲染层始终像素化**：不管上游是文本流还是未来的 Cell Adapter，最终都落到统一的像素渲染管线。
3. **先固定边界，再替换上游**：Renderer 只消费 `DrawList`，不直接依赖 `<pre>` 模型或 Cell Adapter。
4. **兼容层是后续演进，不是当前前提**：M0/M1 的目标是把原生文本流模型跑通，M2 才处理 `row/col -> pixel`。

---

## 当前主路径（M0/M1）

```
┌─────────────────────────────────────────────┐
│                Shell / 应用                 │
└──────────────────┬──────────────────────────┘
                   │ PTY（伪终端）
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
         第一层：协议层（Protocol）
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
┌──────────────────────────────────────────────┐
│  PtyReader                 VtParser          │
│  读PTY字节流         →     解析基础VT/ANSI    │
└──────────────────┬───────────────────────────┘
                   │ TextEvent
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
         第二层：场景层（Text Scene）
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
┌──────────────────────────────────────────────┐
│  TextScene                                    │
│  lines / style spans / cursor / dirty_lines   │
└──────────────────┬───────────────────────────┘
                   │ 脏行 + 字体参数
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
         第三层：排版层（Text Layout）
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
┌──────────────────────────────────────────────┐
│  TextLayoutEngine（`<pre>`模型）              │
│  逐行测量 → 逐字 / 逐grapheme定位 → DrawList  │
└──────────────────┬───────────────────────────┘
                   │ DrawList
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
         第四层：渲染层（Renderer）
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
┌──────────────────────────────────────────────┐
│  GlyphAtlas   Tessellator   RenderPipeline   │
│  字形图集      图元三角化    wgpu提交        │
└──────────────────────────────────────────────┘
                   ↓
              GPU → 屏幕
```

`DrawList` 是当前架构最关键的边界。M0/M1 由 `TextLayoutEngine` 生成它；M2 以后由 `CellAdapter` 生成它；Renderer 不需要知道上游来自哪种布局模型。

---

## M2 兼容路径（未来演进）

```
┌─────────────────────────────────────────────┐
│                Shell / 应用                 │
└──────────────────┬──────────────────────────┘
                   │ PTY（伪终端）
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
         协议层（Protocol）
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
┌──────────────────────────────────────────────┐
│  PtyReader                 VtParser          │
└──────────────────┬───────────────────────────┘
                   │ VtEvent
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
         兼容场景层（Grid Scene）
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
┌──────────────────────────────────────────────┐
│  CellBuffer                                   │
│  cells / cursor / attrs / dirty_rows         │
└──────────────────┬───────────────────────────┘
                   │ 脏行 + 字体参数
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
         兼容排版层（Adapter）
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
┌──────────────────────────────────────────────┐
│  CellAdapter                                  │
│  row/col语义 → 像素坐标 → DrawList            │
└──────────────────┬───────────────────────────┘
                   │ DrawList
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
         同一套渲染层（Renderer）
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

也就是说，M2 不是推翻 M0/M1，而是替换 `DrawList` 的上游。

---

## 各层职责

### 第一层：协议层

**职责**：把 PTY 字节流变成结构化事件。

| 模块 | 输入 | 输出 | 依赖库 |
|------|------|------|--------|
| `PtyReader` | 无（启动 shell） | 字节流 | `rustix` |
| `VtParser` | 字节流 | `TextEvent` / 后续 `VtEvent` | `vte` |
| `GfxParser` | 字节流 | `GfxEvent` | 自研，M4+ |

M0/M1 只需要支持足够跑通日常文本输出的 VT/ANSI 子集，例如：

- 可打印字符
- `CR` / `LF`
- `BS`
- 基本 `SGR` 颜色和样式
- 基本清行 / 清屏

`alternate screen`、精确光标寻址、复杂 TUI 兼容属于 M2 之后的范围。

---

### 第二层：场景层（当前：Text Scene）

**职责**：维护当前阶段最简单、最可用的文本事实来源。

```
TextScene
  ├─ lines: Vec<Line>
  ├─ cursor: Cursor
  ├─ viewport_top: usize
  └─ dirty_lines: BitSet

Line
  ├─ text: String
  └─ spans: Vec<StyleSpan>   // M1 若支持 ANSI 样式
```

M0/M1 这里不是 `CellBuffer`，而是更接近浏览器 `<pre>` 的文本流模型：

- PTY 输出按行保存
- 行内保留字符顺序
- 换行按输入流决定，不做 cell 级列坐标
- 只标记变动的行，供布局和渲染层增量更新

这个模型足以跑通：

- `ls`
- `pwd`
- `tree`
- `git log`
- 普通 shell 提示符和命令回显

---

### 第三层：排版层（当前：TextLayoutEngine）

**职责**：把文本流变成像素坐标下的绘制命令。

```
TextLayoutEngine
  ├─ font_system
  ├─ line_metrics
  └─ line_cache

流程：

  dirty_lines
    → 逐行测量文本宽度
    → 逐字符 / 逐grapheme 累加 x 坐标
    → 计算 baseline / line_height
    → 输出 DrawList
```

M0/M1 的排版模型刻意保持简单：

- 逻辑上等同于 `<pre>`
- 以行作为布局单位
- 不做 `row/col -> pixel`
- 不依赖 pretext
- 不要求 `resize` 后复用 cell 级测量缓存

这层的目标不是“终极排版架构”，而是先把原生文本流模型跑通，并把 Renderer 所需的输入边界固定为 `DrawList`。

---

### 第四层：渲染层

**职责**：把 `DrawList` 变成 GPU 指令。

```
DrawList
  ├─ glyphs: Vec<PositionedGlyph>
  ├─ rects: Vec<RectCmd>         // 背景、selection、光标
  └─ paths: Vec<PathCmd>         // M4+ 图元

Renderer
  ├─ GlyphAtlas
  ├─ InstanceBuffer
  ├─ Optional Path Tessellator
  └─ wgpu RenderPipeline
```

渲染层不关心 glyph 来自：

- M0/M1 的 `TextLayoutEngine`
- 还是未来 M2 的 `CellAdapter`

只要输入仍然是 `DrawList`，渲染层就不需要改架构。

详见 [rendering.md](./rendering.md)。

---

## 线程模型

```
PTY线程                          渲染主线程
─────────────────                ──────────────────────────────
PtyReader                        事件循环（winit）
  ↓                                ↓
VtParser                         接收 channel 事件
  ↓                                ↓
crossbeam::channel  ──────────→  更新 TextScene
                                   ↓
                                 TextLayoutEngine（脏行）
                                   ↓
                                 Renderer.frame()
                                   ↓
                                 wgpu submit + present
```

当前默认只有两个线程：

- PTY 读线程
- 渲染 / 事件主线程

主要状态默认留在主线程：

- `TextScene`
- `GlyphAtlas`
- 布局缓存
- GPU 资源

线程间通信优先用 `channel`。是否需要更激进的并发优化，以 profiling 结果为准，不在 M0/M1 预设复杂共享状态。

---

## Crate 结构

```
stele/
  ├─ src/
  │   ├─ main.rs                // 入口：创建窗口，启动事件循环，启动PTY线程
  │   ├─ pty/
  │   │   ├─ mod.rs             // PTY生命周期管理
  │   │   └─ reader.rs          // PTY读线程
  │   ├─ protocol/
  │   │   ├─ vt.rs              // VtParser（wraps vte crate）
  │   │   └─ graphics.rs        // GfxParser（M4+）
  │   ├─ scene/
  │   │   ├─ text_scene.rs      // M0/M1 当前主场景
  │   │   └─ line.rs
  │   ├─ layout/
  │   │   ├─ text_layout.rs     // `<pre>` 风格文本布局
  │   │   └─ line_cache.rs
  │   ├─ compat/                // M2+ 再引入
  │   │   ├─ cell_buffer.rs
  │   │   └─ cell_adapter.rs
  │   ├─ renderer/
  │   │   ├─ mod.rs             // RenderPipeline，每帧入口
  │   │   ├─ draw_list.rs       // 布局层和渲染层的稳定边界
  │   │   ├─ glyph_atlas.rs
  │   │   ├─ tessellator.rs
  │   │   └─ shaders/
  │   │       ├─ glyph.wgsl
  │   │       └─ primitive.wgsl
  └─ docs/
      ├─ PRD.md
      ├─ architecture.md
      └─ rendering.md
```

---

## 数据流总览

### M0/M1 当前数据流

```
字节流
  → VtParser → TextEvent
  → TextScene.apply(event) → 标记 dirty_lines

每帧：
  TextLayoutEngine.layout(dirty_lines)
    → DrawList

  Renderer.frame(draw_list)
    → GlyphAtlas.get_or_rasterize()
    → InstanceBuffer
    → wgpu submit
    → present
```

### M2 未来数据流

```
字节流
  → VtParser → VtEvent
  → CellBuffer.apply(event) → 标记 dirty_rows
  → CellAdapter.layout(dirty_rows)
    → DrawList

  Renderer.frame(draw_list)
```

两条路径共享同一个 Renderer。

---

## 分阶段实现策略

### M0：窗口 + GPU 文本渲染

- 不接 PTY
- 硬编码几行文本
- 打通 `DrawList -> GlyphAtlas -> GPU`

### M1：PTY + 基础文本流终端

- 接入 shell
- 用 `<pre>` 模型渲染输出
- 跑通 `ls` / `pwd` / `tree` / `git log`
- 支持最小可用的 ANSI 样式

### M2：Cell Adapter 兼容层

- 引入 `CellBuffer`
- 处理 `row/col` 语义
- 为 `vim` / `nvim` / `top` / `htop` 等场景提供兼容路径
- Renderer 尽量不改，只替换 `DrawList` 上游

### M3：基本可用

- 滚动回看
- 选择
- 剪贴板
- 更完整的 VT 行为

### M4：图形协议 v1

- 自定义图形转义序列
- 文本层和图形层同时输出到 `DrawList`

### M5：图层与合成

- 独立图层
- 更细粒度的重绘和缓存策略

---

## 当前非目标（M0/M1）

- 不追求完整 VT 兼容
- 不支持 `alternate screen`
- 不保证 `vim` / `nvim` / `top` / `htop` 可用
- 不实现 cell 级精确光标定位
- 不实现图形协议

这些问题都留到 M2+ 以后处理。
