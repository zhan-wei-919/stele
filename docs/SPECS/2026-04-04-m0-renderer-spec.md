---
type: execution-spec
name: m0-renderer
status: draft
owner: "zhanwei"
created: 2026-04-04
updated: 2026-04-04
tags:
  - renderer
  - gpu
  - m0
---

# M0 渲染层：DrawList -> GPU -> 屏幕

## 1. Purpose

```yaml
problem: Stele 尚无可运行代码。需要先打通渲染管线的最短闭环，验证 GPU 文本渲染 + 子像素抗锯齿的技术路线。
goal: 硬编码若干行文本，经 DrawList -> GlyphAtlas -> Instanced Rendering -> wgpu -> 屏幕 完整渲染，使用 FreeType LCD 子像素光栅化 + dual-source blending。
non_goals:
  - 不接 PTY
  - 不解析 VT/ANSI 转义序列
  - 不实现 TextScene 或 CellAdapter
  - 不实现滚动、选择、剪贴板
  - 不实现矢量图形层
  - 不实现 dual-source blending 降级路径（M1+ 处理）
```

## 2. Scope

```yaml
allowed_paths:
  - src/main.rs
  - src/renderer/mod.rs
  - src/renderer/draw_list.rs
  - src/renderer/glyph_atlas.rs
  - src/renderer/instance_buffer.rs
  - src/renderer/pipeline.rs
  - src/renderer/shaders/glyph.wgsl
  - src/renderer/shaders/rect.wgsl
  - src/renderer/subpixel.rs
  - src/font/mod.rs
  - src/font/rasterizer.rs
  - src/font/discovery.rs
  - Cargo.toml
blocked_paths:
  - src/pty/*
  - src/protocol/*
  - src/scene/*
  - src/layout/*
  - src/compat/*
```

## 3. Target Object / Flow

```yaml
object: Renderer
flow:
  - create_window_and_event_loop    # winit
  - init_wgpu_device                # wgpu adapter/device/surface
  - init_font_system                # fontdb discovery + FreeType rasterizer
  - build_hardcoded_draw_list       # 硬编码文本 -> PositionedGlyph 列表
  - populate_glyph_atlas            # FreeType LCD 光栅化 -> atlas 纹理
  - build_instance_buffer           # PositionedGlyph -> GlyphInstance GPU buffer
  - render_frame                    # render pass: rects -> glyphs (dual-source) -> cursor
  - present                         # surface.present()
```

## 4. Data Structures

```yaml
primary_types:
  - name: PositionedGlyph
    kind: struct
    fields:
      - name: font_id
        type: u32
      - name: glyph_id
        type: u16
      - name: font_size
        type: f32
      - name: pos
        type: "[f32; 2]"
        description: 逻辑像素坐标（左上角）
      - name: color
        type: "[f32; 4]"
        description: RGBA 前景色
      - name: subpixel_offset
        type: SubpixelBin

  - name: SubpixelBin
    kind: struct
    fields:
      - name: x
        type: u8
        description: "0..3，水平方向 4 级量化"
      - name: y
        type: u8
        description: "0..3，垂直方向 4 级量化"

  - name: GlyphKey
    kind: struct
    fields:
      - name: font_id
        type: u32
      - name: glyph_id
        type: u16
      - name: font_size_bits
        type: u32
        description: "f32 to_bits()，用于 Hash/Eq"
      - name: scale_factor_bits
        type: u32
      - name: subpixel_offset
        type: SubpixelBin

  - name: AtlasRegion
    kind: struct
    fields:
      - name: uv_min
        type: "[f32; 2]"
      - name: uv_max
        type: "[f32; 2]"
      - name: size
        type: "[f32; 2]"
        description: 像素尺寸
      - name: bearing
        type: "[f32; 2]"
        description: baseline 偏移

  - name: GlyphAtlas
    kind: struct
    fields:
      - name: texture
        type: "wgpu::Texture"
        description: "Rgba8Unorm，初始 2048x2048"
      - name: packer
        type: ShelfPacker
      - name: cache
        type: "HashMap<GlyphKey, AtlasRegion>"
      - name: current_size
        type: u32
        description: 当前纹理边长

  - name: ShelfPacker
    kind: struct
    fields:
      - name: shelves
        type: "Vec<Shelf>"
      - name: atlas_width
        type: u32
      - name: atlas_height
        type: u32

  - name: Shelf
    kind: struct
    fields:
      - name: y_offset
        type: u32
      - name: height
        type: u32
      - name: x_cursor
        type: u32

  - name: GlyphInstance
    kind: struct
    repr: C
    description: GPU 端 per-instance 数据
    fields:
      - name: screen_pos
        type: "[f32; 2]"
      - name: size
        type: "[f32; 2]"
      - name: uv_min
        type: "[f32; 2]"
      - name: uv_max
        type: "[f32; 2]"
      - name: color
        type: "[f32; 4]"
      - name: bearing
        type: "[f32; 2]"

  - name: RectCmd
    kind: struct
    fields:
      - name: pos
        type: "[f32; 2]"
      - name: size
        type: "[f32; 2]"
      - name: color
        type: "[f32; 4]"

  - name: DrawListOp
    kind: enum
    values:
      - "Insert { line_index: usize, glyphs: Vec<PositionedGlyph> }"
      - "Remove { line_index: usize }"
      - "Replace { line_index: usize, glyphs: Vec<PositionedGlyph> }"

  - name: DrawList
    kind: struct
    description: 渲染层持有，上游通过 DrawListOp 增量更新
    fields:
      - name: lines
        type: "Vec<Vec<PositionedGlyph>>"
      - name: rects
        type: "Vec<RectCmd>"
      - name: cursor
        type: "Option<RectCmd>"

  - name: SubpixelLayout
    kind: enum
    values:
      - HorizontalRgb
      - HorizontalBgr
      - VerticalRgb
      - VerticalBgr
      - None

  - name: Renderer
    kind: struct
    fields:
      - name: device
        type: "wgpu::Device"
      - name: queue
        type: "wgpu::Queue"
      - name: surface
        type: "wgpu::Surface"
      - name: glyph_pipeline
        type: "wgpu::RenderPipeline"
        description: "dual-source blending pipeline"
      - name: rect_pipeline
        type: "wgpu::RenderPipeline"
      - name: atlas
        type: GlyphAtlas
      - name: draw_list
        type: DrawList
      - name: instance_buffer
        type: "wgpu::Buffer"
      - name: dirty
        type: bool

state_enums:
  - name: AtlasState
    values:
      - Ready
      - NeedsRepack
        description: atlas 空间不足，触发倍增 + 全量重打包

relationships:
  - from: Renderer.atlas
    to: GlyphAtlas
    relation: owns
  - from: Renderer.draw_list
    to: DrawList
    relation: owns
  - from: GlyphAtlas.cache
    to: AtlasRegion
    relation: indexes_by_GlyphKey
  - from: DrawList.lines[].PositionedGlyph
    to: GlyphKey
    relation: derives_lookup_key
```

## 5. State Machine

```yaml
states:
  - Uninitialized
  - Ready
  - NeedsRepack
  - Destroyed

key_transitions:
  - from: Uninitialized
    to: Ready
    trigger: "init_wgpu_device 成功 + atlas 纹理创建完成"
  - from: Ready
    to: Ready
    trigger: "apply_ops(ops) 应用 DrawListOp，标记 dirty"
  - from: Ready
    to: NeedsRepack
    trigger: "ShelfPacker 分配失败（atlas 空间不足）"
  - from: NeedsRepack
    to: Ready
    trigger: "倍增纹理 + 全量重打包 + 重建 cache 完成"
  - from: Ready
    to: Destroyed
    trigger: "窗口关闭 / drop"

forbidden_transitions:
  - from: Destroyed
    to: Ready
    reason: "GPU 资源已释放，不可复用"
  - from: Uninitialized
    to: NeedsRepack
    reason: "atlas 尚未创建，不存在重打包"
```

## 6. Module Boundaries

```yaml
owned_paths:
  - src/renderer/*
  - src/font/*
  - src/main.rs
  - Cargo.toml

dependency_direction:
  - from: main
    to: renderer
    rule: allowed
  - from: main
    to: font
    rule: allowed
  - from: renderer
    to: font
    rule: "allowed, 仅通过 rasterizer 接口获取 bitmap"
  - from: renderer
    to: wgpu
    rule: allowed
  - from: font
    to: freetype-rs
    rule: allowed
  - from: font
    to: fontdb
    rule: allowed

forbidden_dependencies:
  - from: renderer
    to: fontdb
    reason: "renderer 不直接做字体发现，通过 font 模块间接获取"
  - from: font
    to: wgpu
    reason: "font 模块不触碰 GPU 资源"
  - from: font
    to: renderer
    reason: "禁止反向依赖"
```

## 7. Input Contract

```yaml
source: "DrawListOp（M0 阶段由 main.rs 中硬编码构建函数生成；M1+ 由上游排版层传入）"
fields:
  - name: op
    type: DrawListOp
    required: true
    valid: "line_index 在 Insert 时 <= lines.len()；在 Remove/Replace 时 < lines.len()"
  - name: glyphs (Insert/Replace 内)
    type: "Vec<PositionedGlyph>"
    required: true
    valid: "font_id 在 font 系统中已注册；glyph_id 对该 font 合法；pos 在逻辑像素合理范围内"
invalid_cases:
  - condition: "line_index 越界"
    error: "panic（debug）/ 忽略（release）"
    handling: "debug_assert 检查边界"
  - condition: "font_id 未注册"
    error: "光栅化失败"
    handling: "用 .notdef glyph 替代"
  - condition: "glyphs 为空 Vec"
    error: 无
    handling: "合法输入，表示空行"
```

## 8. Output Contract

```yaml
returns:
  - name: rendered_frame
    type: "wgpu::SurfaceTexture presented to screen"
    when: "每次 RedrawRequested 事件"
state_changes:
  - object: GlyphAtlas.cache
    from: "可能缺少新 glyph"
    to: "所有可见 glyph 已缓存"
  - object: Renderer.dirty
    from: true
    to: false
  - object: instance_buffer
    from: "旧内容"
    to: "与当前 DrawList 一致"
events:
  - "winit::WindowEvent::RedrawRequested"
logs:
  - "atlas.repack（仅在扩容时）"
  - "frame.glyph_count"
  - "frame.time_us"
observable_side_effects:
  - "像素输出到屏幕"
  - "GPU 纹理写入（atlas 更新）"
  - "GPU buffer 写入（instance buffer 更新）"
```

## 9. Preconditions

```yaml
must:
  - "GPU 支持 wgpu::Features::DUAL_SOURCE_BLENDING"
  - "系统有至少一个可用字体（fontdb 能发现）"
  - "winit 能创建窗口（X11 或 Wayland 可用）"
  - "FreeType 库可链接（libfreetype 已安装）"
```

## 10. Postconditions

```yaml
must:
  - "窗口显示硬编码文本，使用比例字体，子像素抗锯齿可见"
  - "GlyphAtlas 中所有可见字形已缓存，无重复条目"
  - "instance buffer 与 DrawList 内容一致"
  - "静止帧（无 DrawListOp）不触发布局重算或 buffer 重建"
  - "窗口 resize 触发 surface 重建和 redraw，但不触发 atlas 重建"
```

## 11. Invariants Preserved

```yaml
must:
  - "GlyphKey 相同 → AtlasRegion 相同（缓存一致性）"
  - "atlas 纹理尺寸始终是 2 的幂（2048, 4096, 8192...）"
  - "ShelfPacker 分配的区域不重叠"
  - "instance buffer 中每个 GlyphInstance 的 UV 坐标指向 atlas 中有效区域"
  - "SubpixelBin.x ∈ [0, 3] 且 SubpixelBin.y ∈ [0, 3]"
  - "DrawList.lines 的行顺序与屏幕行顺序一致"
  - "Renderer 持有 DrawList 的唯一所有权，外部只能通过 apply_ops 修改"
```

## 12. Budgets

```yaml
constraints:
  - name: draw_calls_per_frame
    limit: "<= 4"
    scope: "单帧（rect pass + glyph pass + cursor pass + 预留 1）"
  - name: atlas_initial_size
    limit: "2048 x 2048 Rgba8Unorm"
    scope: "初始分配"
  - name: atlas_max_size
    limit: "<= 8192 x 8192"
    scope: "单个 atlas 纹理上限"
  - name: glyph_cache_variants_per_glyph
    limit: "<= 16"
    scope: "同一 (font_id, glyph_id, font_size) 的 SubpixelBin 变体上限（4x4）"
  - name: frame_time_target
    limit: "<= 16ms"
    scope: "60fps 目标"
  - name: static_frame_gpu_work
    limit: "仅 render pass 提交，无 buffer 写入、无布局计算"
    scope: "无脏行时的静止帧"
```

## 13. Side Effects

```yaml
allowed:
  - "GPU 纹理写入（atlas 字形上传、atlas 扩容重打包）"
  - "GPU buffer 写入（instance buffer、uniform buffer）"
  - "屏幕像素输出（surface present）"
  - "FreeType 光栅化调用（CPU，仅 cache miss 时）"
  - "fontdb 系统字体扫描（仅初始化时一次）"
forbidden:
  - "文件系统写入"
  - "网络 IO"
  - "PTY 操作"
  - "阻塞主线程超过 frame budget（16ms）"
  - "在 render pass 内部做字形光栅化"
```

## 14. Edge Cases

```yaml
- case: atlas 空间耗尽
  trigger: "大量不同字体/字号/SubpixelBin 变体导致 2048x2048 不够"
  expected: "触发 NeedsRepack，倍增到 4096x4096，全量重打包所有已缓存字形，更新 cache 和 UV"

- case: atlas 倍增后仍不够
  trigger: "4096x4096 仍不够"
  expected: "继续倍增到 8192x8192；超过 max_size 后 panic 并输出诊断信息"

- case: GPU 不支持 dual-source blending
  trigger: "adapter.features() 不包含 DUAL_SOURCE_BLENDING"
  expected: "启动时 panic，输出清晰错误信息：'GPU does not support dual-source blending, required for subpixel text rendering'"

- case: 空 DrawList
  trigger: "初始状态或所有行被 Remove"
  expected: "渲染空白背景 + 光标（如有），不 panic"

- case: 窗口 scale_factor 变化
  trigger: "用户在 HiDPI 和普通显示器之间移动窗口"
  expected: "清空 atlas cache（scale_factor 是 GlyphKey 的一部分，旧缓存自动失效），重新光栅化可见字形"

- case: font_id 指向的字体不存在
  trigger: "fontdb 发现的字体被系统删除"
  expected: "FreeType 光栅化失败，用 .notdef glyph 替代，不 panic"

- case: 零宽字符（ZWJ、combining marks）
  trigger: "DrawList 中 PositionedGlyph 的 glyph_id 对应零宽字形"
  expected: "光栅化得到空 bitmap，不上传 atlas，不生成 GlyphInstance"
```

## 15. Acceptance Cases

```yaml
- id: AC-01
  given: "硬编码 3 行英文文本（含大小写、数字、标点），使用系统默认比例字体，14px"
  when: "启动程序，窗口出现"
  then:
    - "窗口显示 3 行文本，字符间距符合比例字体 metrics"
    - "atlas 纹理格式为 Rgba8Unorm 且 FreeType 以 FT_RENDER_MODE_LCD 光栅化（通过日志验证）"
    - "glyph pipeline 使用 dual-source blending（BlendFactor::Src1）"
    - "背景色为纯黑（或指定深色），文本为白色"
    - "frame_time 日志显示单帧耗时 <= 16ms"

- id: AC-02
  given: "硬编码文本包含 CJK 字符（如「你好世界」），系统已安装 CJK 字体"
  when: "启动程序"
  then:
    - "fontdb 发现 CJK 字体时：CJK 字符正确显示，宽度大于拉丁字符，baseline 对齐"
    - "fontdb 未发现 CJK 字体时：显示 .notdef 方块替代，程序不 panic"

- id: AC-03
  given: "程序已启动并显示文本，窗口处于静止状态（无输入、无内容变化）"
  when: "等待 5 秒"
  then:
    - "事件循环处于 Wait 模式，无 RedrawRequested 事件触发（通过日志计数验证）"
    - "无 queue.write_buffer / queue.write_texture 调用（通过日志验证）"
    - "无 FreeType 光栅化调用（通过日志验证）"
    - "画面保持不变"

- id: AC-04
  given: "程序已启动"
  when: "调用 renderer.apply_ops([Replace { line_index: 0, glyphs: new_glyphs }])"
  then:
    - "仅第 0 行重新构建 GlyphInstance"
    - "其余行的 GlyphInstance 不变"
    - "屏幕正确反映新内容"
    - "新字形（如有 cache miss）已上传到 atlas"

- id: AC-05
  given: "程序已启动"
  when: "用户拖拽窗口边缘 resize"
  then:
    - "surface 重建，文本按新尺寸重绘"
    - "不触发 atlas 全量重建（scale_factor 不变时）"
    - "无闪烁或黑屏"
```

## 16. Pseudocode

```text
fn main():
    window, event_loop = winit::create_window("Stele", 800, 600)

    # wgpu 初始化
    instance = wgpu::Instance::new()
    surface = instance.create_surface(window)
    adapter = instance.request_adapter(features: DUAL_SOURCE_BLENDING)
    if not adapter.features().contains(DUAL_SOURCE_BLENDING):
        panic("GPU does not support dual-source blending")
    device, queue = adapter.request_device(features: DUAL_SOURCE_BLENDING)

    # 字体初始化
    font_db = fontdb::Database::new()
    font_db.load_system_fonts()
    subpixel_layout = detect_subpixel_layout()  # 纯 Rust: wayland/x11/fontconfig
    rasterizer = FreeTypeRasterizer::new(font_db, subpixel_layout)

    # 渲染器初始化
    atlas = GlyphAtlas::new(device, 2048, 2048, Rgba8Unorm)
    glyph_pipeline = create_glyph_pipeline(device, dual_source_blending=true)
    rect_pipeline = create_rect_pipeline(device)
    renderer = Renderer::new(device, queue, surface, atlas, glyph_pipeline, rect_pipeline)

    # 硬编码 DrawList
    hardcoded_text = [
        "Hello, Stele! — Pixel-perfect terminal.",
        "你好世界 — CJK text rendering test.",
        "ABCDabcd 1234 !@#$ mixed content.",
    ]
    ops = []
    for i, line in hardcoded_text:
        glyphs = layout_line(line, rasterizer, font_size=14.0, y=i*line_height)
        ops.push(Insert { line_index: i, glyphs })
    renderer.apply_ops(ops)

    # 事件循环
    event_loop.run(ControlFlow::Wait):
        on RedrawRequested:
            renderer.frame(queue)
        on Resized(new_size):
            renderer.resize(new_size)
            window.request_redraw()
        on CloseRequested:
            exit

fn Renderer.apply_ops(ops):
    for op in ops:
        match op:
            Insert { line_index, glyphs }:
                self.draw_list.lines.insert(line_index, glyphs)
            Remove { line_index }:
                self.draw_list.lines.remove(line_index)
            Replace { line_index, glyphs }:
                self.draw_list.lines[line_index] = glyphs
    self.dirty = true
    window.request_redraw()

fn Renderer.frame(queue):
    if self.dirty:
        # 1. atlas 更新
        for line in self.draw_list.lines:
            for glyph in line:
                key = glyph.to_key(self.scale_factor)
                if not self.atlas.cache.contains(key):
                    bitmap = self.rasterizer.rasterize_lcd(key)
                    match self.atlas.packer.allocate(bitmap.width, bitmap.height):
                        Some(region):
                            queue.write_texture(atlas.texture, region, bitmap)
                            self.atlas.cache.insert(key, region)
                        None:
                            self.atlas.grow_and_repack(queue)
                            # retry allocation after repack

        # 2. instance buffer 重建
        instances = []
        for line in self.draw_list.visible_lines(viewport):
            for glyph in line:
                region = self.atlas.cache[glyph.to_key()]
                instances.push(GlyphInstance {
                    screen_pos: glyph.pos * scale_factor,
                    size: region.size,
                    uv_min: region.uv_min,
                    uv_max: region.uv_max,
                    color: glyph.color,
                    bearing: region.bearing,
                })
        queue.write_buffer(self.instance_buffer, instances)
        self.dirty = false

    # 3. render pass
    encoder = device.create_command_encoder()
    pass = encoder.begin_render_pass(clear: background_color)

    # pass 1: background rects (standard blending)
    pass.set_pipeline(self.rect_pipeline)
    draw_rects(self.draw_list.rects)

    # pass 2: text glyphs (dual-source blending)
    pass.set_pipeline(self.glyph_pipeline)
    pass.draw_indexed(0..6, instance_count=instances.len())

    # pass 3: cursor
    if self.draw_list.cursor:
        pass.set_pipeline(self.rect_pipeline)
        draw_rect(self.draw_list.cursor)

    pass.end()
    queue.submit(encoder.finish())
    surface.present()

fn GlyphAtlas.grow_and_repack(queue):
    new_size = self.current_size * 2
    assert new_size <= 8192
    new_texture = create_texture(new_size, new_size, Rgba8Unorm)
    new_packer = ShelfPacker::new(new_size, new_size)
    new_cache = HashMap::new()
    for (key, old_region) in self.cache:
        bitmap = self.rasterizer.rasterize_lcd(key)  # 重新光栅化
        new_region = new_packer.allocate(bitmap.width, bitmap.height).unwrap()
        queue.write_texture(new_texture, new_region, bitmap)
        new_cache.insert(key, new_region)
    self.texture = new_texture
    self.packer = new_packer
    self.cache = new_cache
    self.current_size = new_size
```

## 17. Execution Plan

<!-- 在 AC 用户确认通过后填写。 -->

```yaml
execution_plan:
  spec_path: docs/SPECS/2026-04-04-m0-renderer-spec.md
  tasks:
    - id: TASK-01
      description: "项目脚手架 + 核心数据结构 + 字体模块"
      owned_paths:
        - Cargo.toml
        - src/renderer/mod.rs
        - src/renderer/draw_list.rs
        - src/font/mod.rs
        - src/font/discovery.rs
        - src/font/rasterizer.rs
      context_sections:
        - Data Structures
        - Module Boundaries
        - Preconditions
      ac_ids:
        - AC-02

    - id: TASK-02
      description: "GlyphAtlas + ShelfPacker + 子像素检测 + WGSL shaders"
      owned_paths:
        - src/renderer/glyph_atlas.rs
        - src/renderer/subpixel.rs
        - src/renderer/shaders/glyph.wgsl
        - src/renderer/shaders/rect.wgsl
      context_sections:
        - Data Structures
        - State Machine
        - Invariants Preserved
        - Budgets
      ac_ids:
        - AC-01
        - AC-04

    - id: TASK-03
      description: "Renderer 核心：wgpu pipeline + instance buffer + frame/apply_ops/resize"
      owned_paths:
        - src/renderer/pipeline.rs
        - src/renderer/instance_buffer.rs
        - src/renderer/mod.rs
      context_sections:
        - Data Structures
        - State Machine
        - Input Contract
        - Output Contract
        - Budgets
        - Side Effects
      ac_ids:
        - AC-01
        - AC-03
        - AC-04
        - AC-05

    - id: TASK-04
      description: "main.rs 集成：winit 事件循环 + 硬编码文本 + 全链路打通"
      owned_paths:
        - src/main.rs
      context_sections:
        - Target Object / Flow
        - Preconditions
        - Pseudocode
      ac_ids:
        - AC-01
        - AC-02
        - AC-03
        - AC-04
        - AC-05
```
