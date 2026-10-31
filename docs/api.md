# 脚本解释库接口

适用基线：CatSystem2 2.6.1.90。库名为 `katze_system2`，容器读取与执行可分别使用。

## 当前范围

|模块|已提供|后续需要补齐|
|:--|:--|:--|
|`expression`|32 位数值、共享／局部变量、命名银行值、优先级、无短路、左值和延迟取值|系统值更新及尚未核实的语法|
|`cst`|容器、类型化记录、预处理、条件、场景调用／返回、直接编号字符串、选项收集和宿主事件|其他内建命令、消息缓存、画面／音频及特殊等待|
|`cstl`|FF 编码、UTF-8 表、姓名／正文片段及选项索引|语言切换、正文回退及历史刷新策略|
|`fes`|容器、混合编码字节保留、注释、重复块、条件、调用、数值赋值、帧等待／重启、宏、对象创建与初始化请求、CIMAGE／IMAGE 状态应用、事件分派、声明和命令事件|其他类型的对象属性、对象操作、其他事件入口、子命令及任务协议|
|`anm`|容器、全部 18 种指令、宿主随机源、变量、条件、跳转及显示属性|时钟缩放、时间追赶及渲染属性组合|
|`kcs`|容器、全部有效指令、整数／浮点／转换、地址读写、窄写入、调用帧、调试元数据及数值／字符串宿主|扩展分配区、系统 API 和任务调度|
|`hg3`|0x300 帧目录、标准元数据及按 ID 选帧，供对象宿主使用|像素解码及其他图像类型|

尚未实现的 KCS 宿主和无效 ANM 指令在执行时返回错误。CST 和 FES 的宿主命令返回事件，由宿主处理；宿主不能通过空实现确认一个尚未支持的命令。成功读取容器不代表该程序的所有执行路径都已实现。

## 数值状态

```rust
use katze_system2::expression::{eval, State};

let mut state = State::new(2048);
eval("#4=10", &mut state).unwrap();
assert_eq!(eval("#4++", &mut state).unwrap(), 11);
```

共享槽数由调用者根据配置提供。`State` 还包含局部区、字符串表、只读命名值表 `named` 和当前参数值 `current`。直接数值入口接受 `#n`、`$n` 和反斜杠局部变量；CST 的 `$` 十六进制转换由其记录读取路径负责。

命名操作数写为 `name` 或 `name[expression]`，省略银行号时读取银行 0；名称区分大小写，缺失绑定返回零。宿主通过 `state.named.entry(name).or_default().insert(bank, value)` 提供数值。FES 使用各自的 `Interpreter.named`，宿主需在相应实例中注册对象标识和更新系统值，调用者的 `State.named` 在 FES 求值前后保持原值。命名值不能作为赋值或自增减的目标；库对这些写法返回诊断。

`Expression::parse` 完整消费输入，解析成功后才允许求值。执行中的除法异常或越界会返回 `Error`；已发生的副作用不回滚。共享状态可在 CST 与 FES 之间复用，FES 实例自行保管独立局部区。

## 容器读取

`cst::Script::parse`、`cstl::Table::parse`、`fes::Program::parse`、`anm::Program::parse` 和 `kcs::Program::parse` 接受完整文件字节。CST 支持压缩和未压缩主体；FES、KCS 按文档读取 Zlib 主体。解压要求完整流结束、长度相符且没有流后附加字节。

FES 可通过 `Program::from_text` 接受已解压字节。声明、文本参数及源码位置保留为字节，不对整个文件统一解码。`Program::diagnostics` 保留孤立条件终止符等结构问题，执行到对应位置时报错。

`katze-script inspect` 读取文件或单层目录并汇总结果。CST 的 items 为记录数，CSTL 为消息条目数，ANM／KCS 为指令数，FES 为文档数。它只检查读取和结构。

## 推进与宿主事件

解释器的 `run` 接收本次指令预算，返回 `Step`：

|结果|调用者职责|
|:--|:--|
|`Event(event)`|完成相应输入、时间或宿主操作|
|`Yield`|本次执行让出；预算耗尽时可再次推进，FES 事件结束时按事件契约恢复主流程|
|`Halted`|当前执行推进已到末尾或停止指令，按所属任务处理|

CST 通过 `SceneSource::load` 获取场景。宿主负责资源查找、名称匹配和加载失败；每个 `Scene` 包含脚本和可选的本地化表。场景调用由解释器保存返回位置。

CST 的姓名／正文事件同时提供 CP932 基础文本和可选的本地化文本，保留空本地化值。正文缺失语言时的最终回退策略交给调用者。直接编号的基础文本字符串替换已执行，本地化文本中的额外替换和控制标记处理仍需接入消息宿主。

CST、FES 的事件在确认前会重复返回。CST 用 `resume` 确认普通事件，选项用 `choose` 按列表位置选择；结果 ID 可以重复。`choose` 转移到该项目标场景，系统变量和界面返回协议仍由后续宿主集成处理。FES 用 `resume` 确认事件，`Frame` 需要等到下一逻辑帧再继续。

FES 内部执行宏声明，初始化按以下顺序交给宿主：

1. `Declarations` 报告 INCLUDE、PRELOAD 和 KEYBLOCK。
2. 逐条 `CreateObject(ObjectDefinition)` 请求创建 DEFINE 对象。
3. 逐条 `InitializeObject(Box<ObjectInitialization>)` 请求应用 OBJECT 属性，每段使用自己的列定义。
4. `Declarations` 报告 SUBCOMMAND。
5. 开始 START 主流程。

`ObjectDefinition.kind` 为基线类型表中的大写名称，`name` 为对象名，`banks` 为有效银行数。`ObjectInitialization` 包含目标引用和已求值的属性；`Option` 区分未指定字段与明确写零，POS 的三个分量分别保留。POS2／SIZE2 保存在 `position_resource`／`size_resource` 中，由宿主读取所指定图像帧的裁剪偏移／裁剪尺寸。资源、图形／音频句柄、父子关系和各类型的属性应用均由宿主负责；当前库提供初始化解码。没有实际属性的行不发出请求。

`fes::ImageReference::parse` 将 POS2／SIZE2 的值拆为资源名称字节和帧 ID。省略帧号时取 0，冒号后读取带可选符号的十进制／十六进制整数前缀，非法前缀取零，不运行表达式。

`hg3::Metadata::parse` 读取 HG-3 0x300 的帧顺序及 stdinfo，保留裁剪尺寸、偏移、完整画布尺寸和绘制基点。`frame(id)` 返回第一个匹配 ID 的帧；ID 与数组下标分别保存。该接口检查帧及标签范围，不解码像素。

`fes::CImageState` 保存 CIMAGE 的坐标、尺寸、颜色、平面引用和显示属性。宿主按具体银行调用 `apply(&attributes, bank)`；此接口仅应用该类型的有效列，不用于 IMAGE 等其他对象。父子对象关系应由宿主先处理，资源查找和绘制也由宿主负责。

`base_point`、`scale`、`rotation` 保存运动可修改的二维基点、缩放及弧度旋转。`subcommand_context()` 提供调用换算所需的当前状态；其中 base 的 Z 分量使用对象优先级。`motion_properties()` 生成运动传递区，`apply_motion_properties(&properties)` 回写位置、基点、缩放、旋转、Alpha、优先级和 display。IMAGE 使用 `base` 上的相同接口。每次新运动更新前，把当前属性交给 `MotionTask::set_properties`；更新完成后读取 `state().properties` 并回写。预算续跑期间维持同一份输入。

`resolve_geometry` 接受提供 `StandardInfo` 的加载闭包，在对象更新阶段先应用 SIZE2 的裁剪尺寸，再应用 POS2 的裁剪偏移。SIZE2 引用持续保留，POS2 在尝试读取时消耗；加载错误直接返回，此前已完成的状态修改保留。原引擎在读取失败时使用零坐标／尺寸，本库保留诊断。

坐标以 f32 保存；一次 POS 更新会将未指定的已有分量也转为整数再写回。`priority` 为绘制优先级：原设置函数在输入与当前值不等时保存输入加 20，相等时保留；`parent_priority` 保存本次 PRI 的输入值。数组递增在这一步之前完成。

`fes::ImageState` 为 IMAGE 保存基础显示状态、资源名、当前图像编号和 32 项帧映射。`apply` 接受按配置转换资源名称的闭包；空 FILE 只清空请求名称，保留当前名称。此阶段保存状态，实际资源加载和像素绘制由宿主在对象更新阶段完成。IMAGE 不应用 CIMAGE 的 SIZE／COLOR 列。

`selected_frame()` 返回当前映射后的帧 ID。初始映射未启用时，IDN 可直接指定任意帧 ID；设置任意 ID 列后启用映射，后续 IDN 仅接受 0～31。一次初始化中先应用 IDN，再更新 ID 映射，因此首次启用映射时仍可能保留范围外的当前编号，此时 `selected_frame()` 返回 0。几何资源通过 `base.resolve_geometry` 接入。

宿主完成请求后用 `resume` 确认，待确认请求重复返回，数量和属性表达式只计算一次。字符串表必须在初始化前准备好，缺失字符串可能使后续参数移动。宿主只应在具备对应实现时确认声明或属性请求。`return_value` 保存最近一次 `break` 的值；不带参数的 `break` 将它重置为 0。

`Interpreter::object_reference(input, state)` 解析对象名及银行表达式。名称精确匹配；省略银行号或值为 −1 时，已知多银行对象保留 −1 表示整个数组，其他对象归一为 0。其他引用的查找和银行范围由宿主处理。KEY_* 中名称为 `0` 的引用解码为空名称和银行 0，表示解绑。对整个数组指定 PRI 时，`sequential_priority` 为真，宿主应按银行号递增优先级。

`fes::parameters` 仅按 FES 语法提取字节词项；`Interpreter::parameters(input, state)` 还展开词项中的字符串变量。宿主用后者读取 `Command` 提供的已展开行，避免重复进行宏替换。Tab、圆括号和双引号的具体边界见 [FES](fes.md)。

宿主处理初始化声明时，可调用一次 `Interpreter::keyblocks(state)` 解码焦点网格。返回 `Vec<KeyBlock>`，`size` 为横向／纵向按钮排列数，`neighbors` 按上、下、左、右保存离开网格后的目标块，−1 表示没有目标。该入口只读取第一段 KEYBLOCK；数量按整数前缀读取，零或负数得到空表。未写条目的尺寸为零、方向为 −1，重复行保留未给出的方向分量，越界编号的行跳过。按钮与网格位置的绑定以及实际键盘／手柄导航由宿主负责。

`Interpreter::subcommands(state)` 返回 `SubcommandTable`，包含按命令名字节索引的绑定和逐行诊断。同名命令保留第一次定义，多个 SUBCOMMAND 段按顺序处理。每个绑定保存类别、原运动资源名、参数描述符及默认文本，以及 `MappedArgument` 映射。`Interpreter::subcommand_arguments(binding, arguments, context, state)` 将去除对象名／命令名的已展开参数换算为运动参数数组。`SubcommandContext` 由宿主按目标银行提供基点、位置、速度、弧度旋转、缩放和 ARGB 颜色；求值使用当前 FES 的局部与命名数值，`@` 为当前属性的整数值。参数编号越界返回诊断。目标资源的配置解析和运动 KCS 启动仍由宿主负责。

`SubcommandParameter.descriptor` 是基线表中区分大小写的名称；省略默认值时保存 `-1`，读取阶段不求值。`MappedArgument::Parameter` 保存映射编号，`Integer` 保存整数前缀，`Context` 保存 cxr／cxg／cxb／cxa／CA 上下文标记。描述参数和映射参数各最多 16 项，默认文本最多 15 字节；越界映射编号需要在实际调用时根据参数表检查。非法行返回带源位置的诊断，其余绑定继续读取。

对象宿主调用 FES `dispatch_event(object, bank, event, default_handler)`，提供对象名称、银行号、事件名称及可选默认事件组。当前支持 `FOCUS`、`UNFOCUS`、`PUSH_L`、`PUSH_R`、`RELEASE_L`、`RELEASE_R`、`SCRL_P` 和 `TOUCH`，返回是否存在处理器。初始化完成后，允许在 `Wait`、`Frame` 或尚无待确认操作时分派；创建、宿主命令、声明尚未完成或另一个事件正在执行时返回错误。

分派后继续调用 `run`，命令通过通常的 `Command` 事件交给宿主，`event_context()` 提供对象与银行号，供宿主解析 `@` 对象命令；数值表达式中的 `@` 已使用银行号。事件完成时 `run` 返回 `Yield`，上下文变为 `None`；下一次 `run` 恢复主流程原等待，或执行事件交出的目标位置。两类事件中的 `next`／`call` 差异详见 [FES](fes.md)。事件中的 `wait`／`exit` 和嵌套分派当前返回错误。

ANM 的 Image／Wait 事件携带持续时间。宿主完成该时长后再调用 `run`；库当前不绑定刷新率或墙上时钟。零耗时跳转循环受指令预算限制。随机区间通过 `run_with_random` 从宿主读取 u32 随机数，固定区间不消耗随机数。`display` 保留当前显示属性，`max_image_id` 由图像宿主设置。

## KCS 宿主

`kcs::Host::call` 接收函数编号、参数字节区和可检查的 VM 存储接口。字符串地址、对象句柄及参数布局依各 native 接口契约解释；库不调用原程序函数指针。

|`NativeReturn`|原状态|行为|
|:--|--:|:--|
|`Yield`|0|让出，无返回值|
|`Value(value)`|1|压入四字节返回值并继续|
|`Continue`|2|无返回值并继续|
|`YieldValue(value)`|3|压值后让出|
|`YieldVoid`|4|无返回值并让出|
|`Retry`|5|恢复参数区的栈位置和调用 PC，让出后重试|

执行器以字节地址管理程序数据、栈和最高位标记的外部存储；全部访问均检查范围。`Program::instructions` 提供只读的解码指令。`Interpreter::debug` 提供当前调试位置、作用域地址和变量索引。当前没有完整任务调度器。

`kcs::CoreHost::new(&mut state)` 在调用者已初始化的 `expression::State` 上提供普通任务表 native 0、1、14、17、23、26、38、41、42、44、45、209～213、276～278、519、789。共享数值区、字符串表和当前参数值可与 CST／FES 复用；209 保留已经存在的状态。789 返回 `configured_shared_slots`，初值为传入状态的槽数，可根据独立配置设置。其他编号返回明确错误，自定义宿主可按编号委托给 `CoreHost`，其余接口自行实现。运动和粒子程序的编号属于其他函数表，调用者必须提供相应宿主。

1 的调试文字保存在 `debug_messages`，调用者可读取并用 `std::mem::take` 取走；库保留字节编码。26 读取展开后的数值前缀，返回 f32 原始位模式；它与要求完整表达式的 213 使用不同契约。

`Interpreter::read_string` 有界读取零结尾字节串，`read`／`write` 用于任意字节。文本 native 保留原始编码，数值表达式需要 ASCII。字符串写入检查可用空间，参数字节数必须与接口契约一致。native 278 原地展开字符串；519 将给定参数写入目标，序号包括命令名且从零开始。

## 约束与验证

单个容器、解压主体和单块运行存储上限为 64 MiB；主要记录表上限为一百万项。表达式最多 64 KiB、512 个节点、128 层递归；参数、菜单扫描和语言表另设 4096 项上限。超限返回诊断。发生执行错误后应丢弃该执行器，修正输入或实现后重新创建。

`Error::offset` 在容器和表达式读取中为字节位置；CST 执行错误使用记录索引，ANM 使用指令索引，FES 使用文本字节位置，KCS 使用代码字节 PC。语法子解析器产生的位置相对于当前表达式或参数。

常规测试仅使用合成字节和脚本，覆盖已核实的数值副作用、条件菜单、场景返回、FES 帧重启、ANM 循环、KCS 调用／重试及非法容器。另已用实际输入批量验证 1286 份容器读取；这项检查不纳入默认测试，也不分发输入材料。

`kcs::ParticleHost` 提供 KCS 2.0 粒子表中的数值、时间、网格、参数快照、源对象查询及退出接口，支持范围和参数见 [KCS 文档](kcs.md#粒子脚本宿主)。调用者在推进前设置时间、当前网格、表面尺寸及投影的 `depth_offset`／`camera_depth`，提供四边形、快照、偏移和标记数组。`ParticleVertex` 保留 XYZ、RHW、颜色和 UV。`ScriptRandom` 保存独立的整数和浮点序列，可用显式种子创建。`ParticleSource` 分别保存源对象的颜色、二维坐标和三维坐标；将查询结果写入 `source`，查询无结果时设为 `None`。`object_id` 为 −1 时使用全零源属性。FES 子命令使用另一张运动表。

`kcs::ParticleTask::new(program, host)` 创建粒子脚本实例，`update(now, budget_per_quad)` 按网格顺序从入口执行，保留共享数据区。未活动时返回 `None`，整轮完成返回 `Some(Step::Halted)`，预算不足返回 `Some(Step::Yield)` 并继续当前网格。预算按单个网格计算；续跑沿用本轮时间，期间应保留宿主时间、网格编号及数组。`origin`、`previous_time` 和整数 `speed` 控制累计时间；网格数组需预先准备且数量相同，可用 `host.reset_quads(count, image_size)` 创建矩形顶点、快照、零偏移和启用标记；该方法保留时间、活动标记和参数。源属性、表面和资源上下文由调用者提供，图形提交与父对象更新由外层负责。

`kcs::MotionHost` 提供运动表的 0～44 项接口，参数契约见 [运动脚本宿主](kcs.md#运动脚本宿主)。用 `MotionState::with_parameters` 创建参数区，以 `external_data` 创建 VM 外部区，执行后用 `MotionState::read` 读取属性及状态。`properties` 保留 36 个原始 DWORD，调用者提供对象属性布局；全部内存访问检查边界。native 38 清除 running 并让出。`MotionTask::new(program, state, base_time)` 创建运动实例；`state()` 读取当前属性及控制数据，`set_properties` 仅替换 36 项属性，保留时间、运行标志、参数和程序数据。

`MotionTask::update(now, host, budget)` 在未到开始时间且 finishing 为零时返回 `None`；否则设置按 u32 回绕的 elapsed，并从入口重新执行，返回 `Some(Step)`。`Halted` 结束本次入口执行，running 的含义由对象调度处理。预算耗尽返回 `Yield`，下一次 `update` 继续同一次执行并保留原 elapsed。宿主事件结束本次更新，下一次更新重新从入口开始。

`finish(host, budget)` 以 elapsed=`u32::MAX`、finishing=1 从入口执行，保留当前 running。完成或错误时恢复这三个字段，保留数据区、属性、参数和宿主副作用。预算让出后继续调用 `finish`；这期间 `update` 返回诊断。`finish` 可以中断尚未完成的普通更新。`Interpreter::reset_execution` 单独提供入口和栈重置，保留数据区、外部区、switch 值及调试元数据。


## 运动队列

`kcs::MotionQueue` 保存按创建顺序执行的运动实例。每次新遍历前，调用者设置 `base_properties`；`speed` 默认为 1。`insert(task, group)` 返回从 1 开始的递增编号，删除及清空后不重置编号。`task(id)` 提供只读任务状态，`group(id)`、`time_origin(id)` 查询分组及时间起点。FES 的两类 SUBCOMMAND 声明在当前基线启动路径均传入分组 0。

`update(now, skip_unstarted, host, budget_per_task)` 从基础属性开始，逐项组合结果。`update_object(now, host, budget_per_task)` 接入对象层的活动查询和统一清理；二维图像每次新更新前提供 `image.base.motion_properties()`，完成后在 `was_updated()` 为真时回写 `properties()`。任务尚未到自身延迟时保留传入属性。

两个更新方法和 `finish(host, budget_per_task)` 均以 true 表示本次遍历完成，false 表示预算让出。预算分别交给每个任务；续跑时调用相同方法，队列保留时间、速度、基础属性快照及已完成的组合结果。`is_pending()` 查询是否需要续跑。`finish` 可中断普通更新，从基础属性开始依次执行提前结束调用，保留队列；结束遍历期间调用更新方法返回诊断。

`any_running(None)` 查询所有任务，`Some(group)` 只查询对应分组。`request_finish(id)` 和 `request_finish_group(group)` 设置结束标志，后者的 None 作用于全部任务；这些请求在下一次更新执行。`set_time_origin(id, origin)` 可指定单个编号或用 None 修改全部起点。`remove(id)` 返回是否找到任务，`remove_group(group)` 返回移除数量，均直接删除。`clear()` 取消预算续跑并丢弃全部任务，保留编号计数和最近结果。预算续跑期间，插入、按编号／分组删除、修改起点和结束请求返回诊断。

`resource::Category { prefix, extension }` 接收配置的虚拟名前缀和缺省扩展名，`resolve(name)` 按基线 CP932 名称规则返回资源字节名。调用者负责类别选择与实际资源查找；完整契约见[资源名称文档](resources.md)。
