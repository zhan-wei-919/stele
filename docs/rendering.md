# Stele 渲染层设计

## 当前定位

渲染层从第一天起只解决一件事：把上游给出的 `DrawList` 高效画到屏幕上。

- M0/M1 上游是 `<pre>` 风格的 `TextLayoutEngine`
- M2 以后上游可以替换成 `CellAdapter`
- Renderer 本身不应该绑定某一种布局模型

这份文档优先描述 M0/M1 可落地的渲染设计，并说明 M2 之后如何平滑演进。

---

## 核心策略

两条原则驱动所有设计决策：

1. **Glyph Atlas**：所有字形打包进 GPU 纹理，避免逐字符上传 bitmap
2. **Instanced Rendering**：文字以实例化 quad 的方式批量绘制，避免逐字符 draw call

补充一条边界原则：

3. **Renderer 只吃 DrawList**：布局和兼容策略都留在上游，Renderer 只做缓存、上传和绘制

---

## 技术选型

| 职责 | 库 | 理由 |
|------|----|------|
| GPU 抽象 | `wgpu` | 跨后端（Vulkan/Metal/DX12），Rust 生态最成熟 |
| 字体发现 | `fontdb` | 扫描系统字体，构建 fallback 链 |
| 字形光栅化 | `swash` | 输出原始 bitmap；Stele 自己掌控布局策略，不绑定外部 layout engine |
| 矢量图元 tessellation | `lyon` | 成熟稳定，SVG path 子集支持完整 |
| 窗口 + 事件循环 | `winit` | 标准选择，Wayland/X11 均支持 |

---

## 渲染层输入边界

Renderer 的稳定输入是 `DrawList`：

```
DrawList {
    glyphs: Vec<PositionedGlyph>,
    rects: Vec<RectCmd>,
    paths: Vec<PathCmd>,      // M4+
    cursor: Option<RectCmd>,
}

PositionedGlyph {
    font_id: u32,
    glyph_id: u16,
    font_size: f32,
    pos: [f32; 2],            // 像素坐标
    color: [f32; 4],
    subpixel_offset: SubpixelBin,
}
```

关键点：

- M0/M1：`TextLayoutEngine` 负责把文本流变成 `PositionedGlyph`
- M2：`CellAdapter` 负责把 `row/col` 语义变成 `PositionedGlyph`
- Renderer 完全不知道 glyph 来自哪种布局模型

---

## Glyph Atlas

### 数据结构

```
GlyphAtlas {
    texture: wgpu::Texture,        // GPU纹理，初始 2048×2048
    packer: ShelfPacker,           // CPU端区域分配器
    cache: HashMap<GlyphKey, AtlasRegion>,
}

GlyphKey {
    font_id: u32,
    glyph_id: u16,
    font_size: OrderedFloat<f32>,
    scale_factor: OrderedFloat<f32>,
    subpixel_offset: SubpixelBin,
}

AtlasRegion {
    uv_min: [f32; 2],   // 归一化UV坐标
    uv_max: [f32; 2],
    size: [f32; 2],     // 像素尺寸
    bearing: [f32; 2],  // baseline偏移
}
```

### 生命周期

```
查询字形
  ├─ 命中 cache → 直接返回 AtlasRegion
  └─ 未命中
       ├─ swash 光栅化 → bitmap
       ├─ ShelfPacker 分配区域
       ├─ wgpu queue.write_texture() 上传
       ├─ 写入 cache
       └─ 返回 AtlasRegion

Atlas 满时：
  → 新建更大的纹理
  → 重新打包已有字形
  → 更新 cache
```

终端常用字符集相对稳定，因此 atlas 的扩容不应成为常态路径。

### Shelf Packing 算法

按行（shelf）分配，同一行高度相近的字形放在一起，空间利用率较高且实现简单。

```
┌────────────────────────────┐ ← shelf 0: 大写字母 (h=16px)
│ A  B  C  D  E  F  G  H ... │
├────────────────────────────┤ ← shelf 1: CJK (h=20px)
│ 你 好 世 界 一 二 三 ...      │
├────────────────────────────┤ ← shelf 2: 小写字母 (h=12px)
│ a  b  c  d  e  f  g  h ... │
└────────────────────────────┘
```

---

## Instanced Rendering

### Instance 数据结构（GPU 端）

```wgsl
struct GlyphInstance {
    @location(0) screen_pos: vec2<f32>,   // 左上角像素坐标
    @location(1) size: vec2<f32>,         // 宽高（像素）
    @location(2) uv_min: vec2<f32>,       // atlas UV左上
    @location(3) uv_max: vec2<f32>,       // atlas UV右下
    @location(4) color: vec4<f32>,        // RGBA
    @location(5) bearing: vec2<f32>,      // baseline偏移
}
```

### CPU 端构建流程

```
DrawList.glyphs
  → 遍历 PositionedGlyph
  → 查 GlyphAtlas 拿 AtlasRegion
  → 构建 GlyphInstance
  → 追加到 Vec<GlyphInstance>

Vec 上传 → wgpu VertexBuffer
一次 draw_indexed_instanced(instance_count) 画完整屏
```

### Vertex Shader 逻辑

```wgsl
@vertex
fn vs_main(
    @builtin(vertex_index) vi: u32,
    instance: GlyphInstance,
) -> VertexOutput {
    let corner = quad_corner(vi);  // (0,0) (1,0) (0,1) (1,1)
    let pos = instance.screen_pos
            + instance.bearing
            + corner * instance.size;
    let uv = instance.uv_min + corner * (instance.uv_max - instance.uv_min);
    // ...
}
```

---

## M0/M1 的缓存策略

当前阶段先围绕“文本流按行变化”做缓存，不引入 cell 级假设。

### 行级 CPU 缓存

```
LineCache[line]:
  glyphs: Vec<PositionedGlyph>    // 该行布局结果
  dirty: bool                     // 来自 TextScene.dirty_lines
```

流程：

```
dirty_lines
  → 只重新布局脏行
  → 更新 LineCache[line]
```

这保证了静止帧不会重复做整屏文本布局。

### 实例缓冲策略

M0/M1 采用保守但清晰的策略：

1. 若没有脏行，直接复用上一帧的实例缓冲
2. 若有脏行，只重新构建受影响的 `LineCache`
3. 然后把当前可见行重新打包成一个连续的 `Vec<GlyphInstance>`
4. 单次 `queue.write_buffer()` 上传

也就是说：

- **先避免重复布局**
- **再考虑更细粒度的 GPU 局部上传**

这比一开始就设计固定 `row * max_cols` 槽位更适合 M0/M1 的 `<pre>` 模型，因为当前阶段并没有稳定的 cell 网格。

### 典型帧的工作量

```
shell 提示符闪烁光标   → 0行脏，复用上一帧文本实例
新增一行输出         → 1行脏，只重新布局该行
大量文本刷屏         → N行脏，重建受影响的可见区域
```

---

## 矢量图形层（M4+）

```
图形转义序列解析 → 图元命令
  → lyon PathBuilder 构建 path
  → lyon FillTessellator / StrokeTessellator
  → Vec<Vertex> + Vec<u32>（三角形列表）
  → wgpu VertexBuffer + IndexBuffer
  → 独立 draw call
```

图元缓存策略：

- 图元内容不变时复用上一帧 tessellation 结果
- 文本层和图形层分别缓存

这条路径不在 M0/M1 当前关键路径上。

---

## 每帧渲染流程（M0/M1）

```
frame(dirty_lines):

  1. 更新脏行布局
     for line in dirty_lines:
       line_cache[line] = text_layout.layout_line(line)

  2. 准备 atlas
     for glyph in visible line_cache:
       atlas.get_or_rasterize(glyph)   // 新字形才触发 swash

  3. 构建实例缓冲（仅有脏行时）
     if dirty_lines not empty:
       instances = pack_visible_lines(line_cache)
       queue.write_buffer(instance_buffer, 0, instances)

  4. Render Pass
     Pass 1: 背景色矩形（selection、高亮、行背景）
     Pass 2: 文本层（instanced glyph quads）
     Pass 3: 图形层（M4+）
     Pass 4: 光标（单独 quad，便于闪烁控制）

  5. surface.present()
```

静止帧：

- 跳过步骤 1 和 3
- 直接复用 atlas 和 instance buffer
- 只重新提交 render pass

---

## M2 如何演进

M2 之后，渲染层尽量不变，只替换上游：

```
替换前：
  TextScene
    → TextLayoutEngine
    → DrawList
    → Renderer

替换后：
  CellBuffer
    → CellAdapter
    → DrawList
    → Renderer
```

保持不变的部分：

- `DrawList`
- `GlyphAtlas`
- `GlyphInstance`
- `RenderPipeline`
- `HiDPI` 和 atlas 缓存策略

可能新增或增强的部分：

- 更细粒度的 dirty row / dirty region 上传
- 更强的背景块、selection、cursor 语义
- 图层与 clipping 规则

---

## HiDPI 处理

- 所有布局坐标使用**逻辑像素**
- `scale_factor`（`winit` 提供）在上传 GPU 前统一乘入
- Glyph atlas 按**物理像素**光栅化，以获得最清晰的输出
- `subpixel_offset` 区分同一字形在不同子像素位置的光栅化结果

---

## 性能目标

| 指标 | 目标 |
|------|------|
| 输入延迟 | `< 8ms`（1 帧内响应） |
| 帧率 | `60fps`，大量文本输出时尽量不掉帧 |
| draw call 数 / 帧 | `< 10` |
| glyph atlas 命中率 | 日常使用 `> 99%` |

M0/M1 不追求一次性把所有优化做满，但要求缓存边界正确，后续能继续向下优化。

---

## 并发模型

默认原则：

- PTY 读取放后台线程
- 布局缓存、atlas、GPU 资源留在渲染主线程
- 线程间通信优先用 `crossbeam::channel`

当前不预设复杂共享状态，也不把“绝对无锁”当作硬目标。M0/M1 的重点是：

- 热路径尽量单线程独占
- 不在布局和渲染缓存之间来回拷贝
- 有明确 profiling 证据后再升级并发策略

---

## 参考实现

- [Zed GPUI 渲染层](https://github.com/zed-industries/zed/tree/main/crates/gpui) — glyph atlas 和 batching 的参考
- [Alacritty renderer](https://github.com/alacritty/alacritty/tree/master/alacritty_terminal) — 终端 GPU 渲染参考
- [wgpu 示例](https://github.com/gfx-rs/wgpu/tree/trunk/examples) — `wgpu` API 用法
