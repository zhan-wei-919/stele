# Stele 输入链路分层设计

## 背景

`stele` 已经把平台键盘事件收敛成纯输入事实：

- `InputEvent`
- `KeyEvent { code, modifiers, kind }`
- `KeyCode::Char(char) | Enter | Backspace | Left | Right | ...`

这一步解决了 IO 层职责边界问题：IO 只报告“发生了什么输入事实”，不再直接承载文本提交语义。

但在当前实现里，输入事实进入 store 后，仍然会被 reducer 直接解释。典型例子是默认滚动逻辑直接在 reducer 里匹配 `Up/Down/PageUp/PageDown/Home/End`。

这会把三类职责混在一起：

- 输入事实
- 输入语义
- 状态变更

后续如果要支持文本输入、focus-aware shortcut、overlay/modal 拦截、多个交互目标并存，这个链路会迅速失控。

因此，`stele` 后续的输入系统应当显式分层。

## 核心思想

这套分层的核心不是“多加几层抽象”，而是把三件事严格拆开：

1. 输入事实：平台上到底发生了什么
2. 输入语义：这次输入在当前上下文里是什么意思
3. 状态变更：这个语义最终如何修改模型和触发重算

用一句话概括：

**fact != intent != state change**

在 `stele` 里，这意味着：

- IO / event 层只产生 `InputEvent`
- 中间层把 `InputEvent` 解释成 `Command`
- reducer 只消费 `Command`
- compose 层只处理失效和重建

因此，后续的目标链路应为：

`InputEvent -> Context -> Command -> Reducer/AppLogic -> Invalidation -> Compose`

## 分层设计

### 1. 输入事实层

职责：

- 从平台事件归一化出统一输入模型
- 不负责业务语义
- 不负责状态更新

当前对应：

- `src/io/`
- `src/event/handlers.rs`
- `src/event/router.rs`

产物：

- `InputEvent::Key(KeyEvent)`
- `InputEvent::Mouse(MouseEvent)`
- `InputEvent::Paste(String)`（已预留）

这一层的目标是“稳定、纯净、可传输”。它应该只表达事实，不表达解释。

### 2. 上下文层

职责：

- 回答一句话：**这次输入应该由谁来解释**

它不是一个复杂子系统，也不是 reducer 的替代品。它只做输入归属和优先级判定。

最小设计目标：

- 允许未来支持多个输入目标
- 允许未来支持 modal / overlay / focused widget
- 当前阶段保持为薄抽象

推荐最小表示：

```rust
enum InputContext {
    Global,
    Viewport,
    TextInput(TextInputId),
    Overlay(OverlayId),
}
```

如果当前系统暂时只有单一输入目标，这一层可以先退化成固定值，例如始终返回 `InputContext::Viewport`。  
它存在的意义不是立即承载复杂逻辑，而是给未来扩展留出稳定边界。

### 3. 语义解释层

职责：

- 将 `InputEvent + InputContext` 解释为 `Command`
- 先做 keybinding / command 解析
- 再做 fallback 文本输入解释

这一层是输入系统的核心语义层。它将“按了什么键”转成“应用应该做什么”。

推荐输出示例：

```rust
enum Command {
    ScrollByLine(i32),
    ScrollByPage(i32),
    ScrollToStart,
    ScrollToEnd,
    InsertChar(char),
    InsertText(String),
    DeleteBackward,
    DeleteForward,
    MoveCursorLeft,
    MoveCursorRight,
    CopySelection,
    PasteText(String),
    DismissOverlay,
}
```

这一层的关键原则：

- reducer 不直接消费 `KeyEvent`
- shortcut 和 text input 在这里分流
- 同一个 `KeyEvent` 的最终语义，必须由上下文决定

例如：

- `Ctrl+C + TextInput` -> `CopySelection`
- `Char('a') + TextInput` -> `InsertChar('a')`
- `Down + Viewport` -> `ScrollByLine(1)`

### 4. reducer / app logic 层

职责：

- 只消费 `Command`
- 修改模型、交互状态、focus 状态等
- 不直接理解平台输入

这一层是应用语义状态机。  
它不该再知道 `KeyCode::Down` 或 `KeyCode::Char('a')`，而只关心 `Command::ScrollByLine(1)` 或 `Command::InsertChar('a')`。

这会带来两个直接收益：

- reducer 的职责更单一
- 同一条命令可以由多种输入方式触发

例如：

- 滚轮滚动和 `PageDown` 都可以产出滚动命令
- 粘贴命令和菜单操作都可以产出 `InsertText`

### 5. 失效层

职责：

- 明确一次命令执行后需要什么级别的重算

当前 `stele` 只有粗粒度的 `Changed / NoChange` 思路，但后续应逐步演进为更清晰的失效分类。

推荐方向：

```rust
enum Invalidation {
    None,
    InteractionOnly,
    Recompose,
    ReprepareAndCompose,
    ResetAtlasAndCompose,
}
```

这层的作用是避免把“状态更新”和“渲染代价”混为一谈。

例如：

- 纯滚动：`Recompose`
- 文本内容变化：通常至少 `ReprepareAndCompose`
- scale factor 变化：可能 `ResetAtlasAndCompose`

### 6. Compose 层

职责：

- 根据 invalidation 决定是否 prepare / compose / rebuild
- 不负责输入语义
- 不负责业务状态机

这层继续保留在现有 `store/runtime + composer` 结构中即可。  
它应该是输入系统的末端，不是语义系统的起点。

## 为什么上下文层只做薄抽象

这次设计故意不把上下文层做重，原因有三点。

### 1. 当前系统还没有多个成熟输入目标

现在的 `stele` 主要仍是 viewport 驱动。  
如果一开始就为 context 引入过多结构，会先得到一层“没有真实使用者的抽象”。

因此当前最合理的做法是：

- 保留上下文边界
- 让它先足够薄
- 等文本输入框 / overlay / modal 真正落地后再逐步长出复杂度

### 2. 真正复杂的是 command 解释，不是 context 本身

上下文层的职责非常小：

- 识别当前输入目标
- 处理归属优先级

真正复杂的部分在于：

- shortcut 如何解析
- 文本输入如何 fallback
- 命令如何影响状态

所以当前阶段应该优先把复杂度放在 `InputEvent -> Command` 上，而不是 context 本身。

### 3. 未来需要扩展，但现在不需要预支抽象成本

这次做减法，不意味着否认未来需要 context。  
恰恰相反，这里保留一个薄抽象，就是为了将来不需要再拆一次链路。

目标是：

- 现在不过度设计
- 未来不推倒重来

## 推荐接口形状

当前阶段推荐的最小接口如下：

```rust
fn resolve_input_context(model: &Model, interaction: &InteractionState) -> InputContext;

fn resolve_command(
    context: InputContext,
    event: &InputEvent,
) -> Option<Command>;

fn apply_command(
    model: &mut Model,
    interaction: &mut InteractionState,
    command: Command,
) -> Invalidation;
```

这个接口有三个明确边界：

- `resolve_input_context` 只做归属判断
- `resolve_command` 只做语义解释
- `apply_command` 只做状态更新

如果当前阶段想进一步做减法，也可以临时把前两步合并：

```rust
fn resolve_command(
    model: &Model,
    interaction: &InteractionState,
    event: &InputEvent,
) -> Option<Command>;
```

但在语义上仍然要坚持：

- context 只是其中一个判断维度
- reducer 不直接消费原始 key fact

## 与当前 `stele` 的映射关系

当前状态：

- `EventRouter` 负责输入归一化
- `Store` 直接把 `InputEvent` 交给 `Reducer`
- `Reducer` 直接解释部分按键

目标状态：

- `EventRouter` 继续负责输入归一化
- store 在 reducer 之前先做一次 `InputEvent -> Command`
- reducer 只处理 `Command`

也就是说，第一步不是“全面重写输入系统”，而是先把当前 reducer 中的键盘解释逻辑提出来。

最自然的起点是把以下逻辑命令化：

- `Up` -> `ScrollByLine(-1)`
- `Down` -> `ScrollByLine(1)`
- `PageUp` -> `ScrollByPage(-1)`
- `PageDown` -> `ScrollByPage(1)`
- `Home` -> `ScrollToStart`
- `End` -> `ScrollToEnd`

这一步完成后，链路就从：

`InputEvent -> Reducer`

演进为：

`InputEvent -> Command -> Reducer`

然后再逐步把 context 和文本输入加进来。

## 演进顺序

推荐按以下顺序演进：

### Phase 1：命令化现有滚动输入

- 引入 `Command`
- 把 reducer 内现有键盘滚动逻辑提取到解释层
- reducer 只消费滚动命令

### Phase 2：引入薄的 context 边界

- 加入 `InputContext`
- 当前先退化成 `Viewport`
- 保持接口稳定

### Phase 3：支持文本输入目标

- 引入 `TextInput` 作为上下文目标
- 添加 `InsertChar / InsertText / DeleteBackward / MoveCursorLeft` 等命令
- 文本输入不再直接依赖 `KeyEvent` 以外的隐式行为

### Phase 4：支持独立文本提交事件

- 将 `Paste(String)` 接入平台事件
- 未来需要时新增 `TextCommit` / `ImeCommit`
- 继续保持 `KeyEvent` 只表达按键事实

## 非目标

这份设计文档明确不追求以下内容：

- 在当前阶段引入完整 keybinding 配置系统
- 在当前阶段实现 IME / preedit / composition
- 在当前阶段把 context 扩展成复杂运行时框架
- 让 reducer 同时承担输入解释和状态更新两类职责

## 总结

`stele` 输入系统后续的核心方向是：

- **IO 层纯事实**
- **上下文层薄抽象**
- **语义层产出命令**
- **reducer 只消费命令**
- **compose 层只处理失效后果**

最关键的判断标准不是“有没有更多层”，而是：

**任何一层都只做一件事，并且不跨层偷做解释。**

如果后续实现仍然让 reducer 直接匹配 `KeyCode`，那么这套分层就还没有真正成立。
