# 脚本解释库接口

适用基线：CatSystem2 2.6.1.90。库名为 `katze_system2`，容器读取与执行可分别使用。

## 当前范围

|模块|已提供|后续需要补齐|
|:--|:--|:--|
|`expression`|32 位数值、共享／局部变量、命名银行值、优先级、无短路、左值和延迟取值|系统值更新及尚未核实的语法|
|`cst`|容器、类型化记录、预处理、条件、场景调用／返回、直接编号字符串、选项收集和宿主事件|其他内建命令、消息缓存、画面／音频及特殊等待|
|`cstl`|FF 编码、UTF-8 表、姓名／正文片段及选项索引|语言切换、正文回退及历史刷新策略|
|`message`|正文标记、颜色／字号、禁则／分组／注音布局、逐字时间与页状态、显式字形、轮廓／阴影及持久普通／渐显显示、消息任务、默认配置、普通文字调用及画布提交|设备字体适配、其余文字命令及宿主等待|
|`audio`|声音任务、循环点、Vorbis 解码、通道装载／属性及整段／流式缓冲区播放|淡变、混音、设备及其他声音命令|
|`settings`|四块全局配置、基线初值、XML 字段覆盖、音量换算及普通 359／360 的 VM 读写|配置自动装载及媒体服务应用|
|`language`|主语言／消息语言前缀、默认回退、普通 1062 字符串替换与 FES 图像文件刷新|语言配置自动装载、平台语言识别及切换命令|
|`save`|CSS 加载、编号记录、容量／标签、普通容器 native、250 共享值恢复、任务目录读取及修改时间服务、FES 最新槽号查询|文件写入、其他数值分组与对象恢复、嵌套容器及平台目录发现|
|`fes`|容器、混合编码字节保留、注释、重复块、条件、调用、数值赋值、帧等待／重启、宏、对象创建与初始化请求、CIMAGE／IMAGE／CPLANE／PLANE 状态应用、事件分派、声明、子命令绑定／换算／调用事件|其他类型的对象属性、对象操作、其他事件入口、运动资源装载及子任务协议|
|`anm`|容器、全部 18 种指令、随机源、时间调度、重启、图像动画装载及属性组合|资源名称请求、图像选帧及主循环衔接|
|`kcs`|容器、全部有效指令、地址／类型运算、调用帧、调试元数据、普通任务／消息／跨任务服务包装、数值／字符串／集合／XML／图像数据宿主、运动与粒子宿主|扩展分配区、其余普通 native 及图形／音频等具体服务|
|`resource`|启动归档类别配置、名称补全、目录索引、KIF 解密及虚拟名路由|自动挂载、列表读取、补丁发现、缓存及别名|
|`hg3`|0x300 帧目录、标准元数据、按 ID 选帧及原生 32 位完整帧像素解码|其他位深、切片及其他图像编码|
|`graphics`|BGRA8 渲染面、裁剪填充／复制／普通混合，图像装载与持有、绘图变换／投影／三角形及排序、KX2 几何装载、显式画布图元、画布矩形／源变换／延迟切割及独立帧提交、基础二维任务及图元脚本接口、纯色／渐变三角形和纹理 CPU 核心、分块渲染目标、绘图状态及画布淡入淡出任务|完整帧自动驱动、缩放／特殊混合、输入绑定、缓存和设备呈现|
|`input`|两级双向坐标转换、区域和键命令、物理按钮配置映射、四组扫描与重复、捕获、位图及图像 Alpha 生成／绑定、指针通知、滚轮、移动阈值、平滑定位、自动光标、快照查询；对象、显式入口及同步设备更新|平台设备采集、配置自动装载及其他导航路径|
|`task`|任务编号、父子索引、子树移除、编号回收、直接命令与类型广播路由、基类控制、整数时钟及同步树更新／销毁调度|类型专属对象与序列化|
|`runtime`|任务树、对象存储、普通宿主状态与对象工厂的整合；普通对象、基类对象、任务 native、78／79／80／82／83／84／88／92／100／101／104／105／848／849、111／112／114／115／117、256／258／260／262／263／342、365／366／368、447～449、452、590～597、630、645／646、739|其他对象类型、系统 native 组合、程序装载时点|

尚未实现的 KCS 宿主和无效 ANM 指令在执行时返回错误。CST 和 FES 的宿主命令返回事件，由宿主处理；宿主不能通过空实现确认一个尚未支持的命令。成功读取容器不代表该程序的所有执行路径都已实现。

消息文本可通过 `message::Markup::parse(text, &defaults)` 解析，输入已解码为 Unicode，字体尺寸与三种默认颜色由调用者提供。输出保留逐字符样式、分组及 UTF-8 来源区间。`Markup::layout(&settings, measure)` 使用字宽闭包完成换行、禁则、分组、悬挂缩进、对齐及注音定位；设置保留 CP932／Unicode 对象路径，`BreakCharacters::for_encoding` 提供对应默认集合。`Markup::schedule(&layout, &timing, initial_units)` 将等待、字间隔及注音关联换算为逐记录显示时间，返回下一片段可使用的累计计数。`message::Page` 提供延后解析的追加、整页准备、布局失效和清空，保留样式、分组编号、旧自动换行及显示时间；失败后可重试。`Page::advance` 推进普通逐字显示并返回首次显示的布局下标及当前范围完成状态，`remaining` 查询尚未显示的字形数量，`set_speed` 切换速度并维护时间基准。字体配置及宿主等待继续接入，详见 [消息标记接口](message.md)。

`message::Font::from_bytes(bytes, collection_index)` 提供显式字体的整数度量与六十四级灰度遮罩，不查询系统字体或自动处理缺字回退。`GlyphMask::colorize` 使用消息颜色及基线覆盖表生成 BGRA 像素。`Page::paint_foreground(&mut surface, origin, glyph)` 将已显示的正文／注音前景混合到目标，调用者负责重绘背景。`GlyphEffects` 提供轮廓与偏移阴影，`GlyphMask::paint` 合成单个字形，`Page::paint` 将字形效果连接到已显示的正文与注音；保留阴影独立颜色顺序和小字号偏移规则。字体配置及宿主等待继续接入，具体约束见[字形与前景绘制](message.md#显式字体与前景绘制)及[字形效果](message.md#轮廓偏移阴影与字形合成)。

`message::Reveal` 提供水平渐显的过渡表、原始时间换算、整面颜色同步和逐字形 Alpha 更新，来源保存完整像素，目标持续保留显示状态。`Layout::reveal_starts` 生成跨行起点，`RevealGlyph` 组合起点、消息面位置和局部绘制范围；接口约束见[水平渐显核心](message.md#水平渐显核心)。持久页面推进见下文的 `Display`，脚本完成等待继续接入消息对象。

`message::Display::new(defaults, encoding, size, settings)` 持有消息页与持久像素，串联追加、准备、普通逐字／渐显、效果绘制及清空；`advance` 返回首次处理、首次完成的布局下标和当前范围完成状态。`surface()` 可直接合成到最终画面，`page()` 提供只读布局与 `GlyphState` 查询，`source_surface()` 查询可选的完整渐显来源。`dirty_rectangle()` 查询累计更新范围，`clear_dirty()` 显式清除登记；`clear()` 保留既有登记。`reconfigure(defaults, size, settings, &layout, &timing, measure)` 按新配置重解析完整来源，准备成功后重建像素、清空脏区与显示进度并保留速度；失败时原对象保持。服务失败后保留已处理状态并可重试，详见[持久消息显示](message.md#持久消息显示)。消息任务见下段，输入翻页及脚本等待继续接入。

`message::MessageFactory { fallback }` 创建类型 48／63 的 `MessageObject`，复用平面属性与生命周期，接入默认配置、配置、追加、分隔、逐字／渐显、强制显示、速度、剩余数量和画布提交。调用前通过 `Runtime::set_fonts` 绑定 `FontServices`，通过 `host_mut().message_time_scale` 配置消息全局时间比例，按需设置 `host_mut().message_defaults = Some(MessageDefaults { .. })`；独立宿主提供相应字体、默认配置、可见性与提交服务。原配置使用显式 `CommandBuffers`，不解引用进程指针。运行时接入普通 611～619、695 和 884，配置地址可来自 VM 普通区或外部区，数量查询要求有效类型 48 对象。具体契约见[消息任务对象](message.md#消息任务对象)和[普通 KCS 调用](message.md#默认配置与普通-kcs-调用)；CST 消息及完成等待继续接入。

FES `fontcom add name` 调用同一字体服务的 `add_resource`，名称为已展开的 CP932 直接文件名；缺失／空名称跳过。返回 false 时继续脚本，服务错误保留当前事件。默认方法返回未支持诊断，宿主应实现实际字体注册；其余字体枚举及默认名称命令继续诊断。

`audio::parse_loop_table(bytes)` 读取循环点表，保留待补全的字节名称及原始 64 位起点／终点。`LoopPoints::register(name, start, end)` 保存声音命令 199 的原始循环范围。首次登记追加记录；同名更新只改起点，保留原终点、名称与顺序。`find(name)` 和 `entries()` 提供只读查询，名称及范围限制见[声音接口](audio.md)。当前接口不解码音频，也不执行播放。

`audio::SoundConfig` 保存应用设置后的 `SoundPart` 分类树、可选主音量及主静音。节点分别保存分类名称、文件前缀、编号、音量、播放／系统语音开关、静音及有序子节点。`system_voice_candidates(paths)` 提供整树展开或显式路径筛选，`system_voice_enabled(path)` 查询原始开关，`volume(path)` 查询当前音量；`volume_by_name(name)` 按名称查首个分类，精确小写 master 使用显式主音量。宿主通过 `HostState::sound_config` 及 `sound_config_enabled` 提供配置和启用字段；FES `sysvoiceex` 已连接共享数值槽与共享整数随机流，用于选择配置编号，`getsndconfex` 已提供 `volume_id`／`sysplay_id` 两种编号路径查询，以及 `volume name` 的分类名称／主音量查询。配置装载、筛选与随机消耗规则见[声音接口](audio.md#系统语音配置选择)。

`audio::SoundGroups` 独立保存 `SoundGroup { id, label, range }`，由 `HostState::sound_groups` 提供。210 通过 `Host::sound_group_id` 查询当前全局标签；Runtime 创建声音任务时捕获编号范围，同编号首项保留，后续标签修改不改变已有任务范围。`SoundState::set_groups`／`group_range` 提供显式配置和范围查询，`SoundChannel::is_grouped` 查询装载标记。206 从组内选择物理通道，成功返回通道编号，缺组或缺资源返回 u32::MAX；非零 mode 使用加 32 的物理通道并关联自身。名称、容量、回收与失败状态见[声音组与分组装载](audio.md#声音组与分组装载)。

`SoundConfig::file_part(name)` 按非空文件前缀、先父后子查首个匹配；`file_volume`／`file_play_enabled`／`file_system_voice`／`file_mute` 处理各字段的缺省值，`master_mute()` 查询主静音。`group_volume(name)` 单独按分类名称查询，master 在这里是普通名称。规则与缺省值见[文件分类与播放设置](audio.md#文件分类与播放设置)。`HostState::sound_settings_dirty` 保存外层管理的媒体属性刷新标记。

`audio::decode_vorbis(bytes)` 返回保留采样率和声道顺序的 `audio::Pcm`，通过 `frames()`、`samples()` 查询完整采样帧和交错 i16 数据。完整逻辑流、容量和结束页规则见[PCM 与解码](audio.md#pcm-与-vorbis-解码)。

FES SOUND 文件请求通过 `Objects::set_sound_task` 的绑定发送 180／182／201／206／210，新配置下属性刷新依次发送 187 音量与 190 声像；未绑定的独立对象更新向根 35 查询。`fes::SoundState::channel_result()` 查询最近装载后的原始结果。场景媒体更新按文件播放／系统语音开关发送播放、停止及首次毫秒命令，同步执行对象自身 PLAY 处理器；`started_tick()` 查询对象首次毫秒。仅持有 Objects 的更新在需要回调时要求所属场景。缓冲区、请求消费及失败契约见[声音媒体提交](audio.md#文件请求的媒体提交)与[播放动作](audio.md#播放动作与-play-处理器)；组通道自动回收、淡变和设备继续接入。

`audio::SoundFactory { enabled, fallback }` 创建类型 13 声音任务。`SoundObject::audio()` 及 `TaskObject::audio_state()` 查询 `SoundState`，其中 `loop_points()` 为循环范围，`channel_time(0..64)` 为原始空闲通道累计时刻，`channel(0..64)` 借用物理通道的 PCM、名称、装载参数和关联。180 装载单个 Ogg Vorbis 来源，非零 mode 使用加 32 的备用通道；182 按普通／分组状态处理自身及关联资源，187／188 设置及查询资源音量，190／191 设置及查询资源声像，201 查询时长毫秒。运行时普通 400 使用资源服务按 sound 类别补全／读取表，经根 35 查询目标并逐条发送 199；没有返回值。资源提供者负责声音分类配置，未接入的播放命令返回诊断。禁用、删除、缓冲区、附加资源和部分失败契约见[声音任务](audio.md#声音任务)及[普通装载](audio.md#普通-kcs-装载)。

`SoundChannel::requested_volume()` 查询任务保存的 f32 音量目标，`volume()`／`pan()` 返回物理通道已装载资源的原始属性，空资源为 None。`Host::sound_stereo()` 默认一，`HostState::sound_stereo` 可覆盖；此开关只门控声像应用，查询保持。命令的关联选择、范围和返回槽约束见[声音任务](audio.md#声音任务)。

声音任务 183 开始整段或流式缓冲区播放，184 暂停、185 停止、186 查询播放开关，218 登记首次系统毫秒，219 修改播放次数。`SoundState::read_channel(channel, frames)` 按源格式读取物理通道 PCM 并推进播放位置；`TaskObject::audio_state_mut()` 提供可变存储。`SoundChannel::is_playing`／`position_frames`／`remaining_plays`／`completed_passes`／`started_tick` 提供相应查询，`Host::system_time_ms()` 读取共享毫秒时刻。关联、次数、输入范围、结束与未接入路径见[整段缓冲区播放](audio.md#整段缓冲区播放与采样推进)及[流式预取与循环](audio.md#流式预取与循环)。流式位置为环形缓冲内的采样帧位置，次数在预取时扣减。淡变、混音及设备输出继续接入。

普通 938 直接返回 `HostState::sound_config_enabled`，不读取参数或归一化结果。该字段默认零，表示新声音分类配置的启用状态，由配置宿主同步提供，独立于声音任务的设备门控。

`settings::SystemSettings` 保存 sound／message／effects／window 四块基线配置，以及两组各八项的局部／全局变量范围、`auto_select`、`fullscreen` 和 `fullscreen_mw_pos`。`Default` 提供构造初值，`apply_xml(bytes)` 覆盖已有用户设置字段，`apply_startup_xml(bytes)` 覆盖 `GAME/flag` 中的范围；两者均覆盖 `GAME/autoselect` 并保留缺失值，应按启动、游戏默认、用户设置的顺序应用。`refresh_sound_volumes()` 显式重算四路有效音量。准备后设置 `CoreHost::system_settings` 或 `Runtime::host_mut().system_settings`。普通 359 依序复制四块到 VM，360 从 VM 写回前三块；两者不压值。359／360 的地址零有效，起点在区外的地址跳过，合法起点的容量不足继续诊断。790 按组和项索引查询变量范围，依序写入起始编号及数量，返回是否找到。999 忽略参数并返回 `auto_select` 的原始 DWORD。741 在 `fullscreen` 非零时返回 `fullscreen_mw_pos`，为零时返回零；启动读取 `SCREEN/mw_pos`，用户设置读取 `WINDOW/fullscreen` 和 `FULL_SCREEN/mw_pos`。自动装载和媒体应用尚待接入，具体大小、XML 范围与部分失败契约见[全局设置](settings.md)。

`language::LanguageState` 保存 CP932 字节前缀表、默认编号和两套选择，通过 `CoreHost::languages` 或 `Runtime::host_mut().languages` 提供。`expand(input, mode)` 按 mode 零替换 `%LANG%`、一替换 `%LANG_M%`；其他值保留输入。选择越界时使用默认编号，默认也越界时保留标记。替换区分大小写、覆盖全部匹配，输入及前缀在首零结束。前缀最多 255 字节，有效替换的输出最多 2047 字节，超限返回诊断。普通 1062 接受 mode 和 VM 字符串地址，原地写回并保留返回槽；无效 mode 不访问地址。宿主负责把配置中的 Unicode 前缀转为 CP932，自动装载及平台语言选择尚未接入。

`expand_image(input)` 先替换主语言，再替换消息语言。运行时的 FES 初始化使用此规则，语言状态改变后在下一次媒体更新前刷新该场景的 IMAGE／BUTTON 来源；只有转换后文件名变化才请求重建。类别名称补全、预加载名称及其他资源读取不自动替换语言标记。

## CSS 数据容器

`save::DataContainer` 提供 CSS 加载及编号记录的追加、首项读取／删除与清空。`Runtime::set_save_services` 或 `CoreHost::save_services` 提供按任务存档目录读取文件的服务；247 展开文件名后调用该服务。238／244／246／248／254／389／469 操作同一份持久容器存储，任务销毁和服务替换不回收容器；246 显式释放，失效或类型错误的句柄诊断。接口及 CSS 长度／解压约束见[CSS 数据容器](save.md)。

`save::SaveServices::modified_time(task_id, name)` 接收相对存档名称的 CP932 字节，返回可选的 Windows FILETIME 计数；默认实现返回未接入诊断。FES `newsave destination first last` 用该接口选择最新槽号，预算续跑和服务错误保留已求值参数、已完成文件及当前选择，结果在扫描完成后写入共享变量。空范围不调用服务，目的编号越界仍查询全部文件。文件名、时间比较及范围约束见[槽号时间查询](save.md#fes-槽号时间查询)。

运行时普通 250 查询目标任务命令 177，将指定记录的整四字节值写入共享区，从指定槽位起裁剪至区域末尾。命令以 `CommandBuffers::data1` 的四字节可用标记选择当前运行时共享区，支持执行中普通对象；零句柄、缺失记录及未提供数值区分别保留其提前返回顺序。成功标记不取决于复制槽数。

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
|`Paused`|KCS 普通恢复被执行模式暂停，本次调用已完成；消息入口按独立契约处理|
|`Halted`|当前执行推进已到末尾或停止指令，按所属任务处理|

CST 通过 `SceneSource::load` 获取场景。宿主负责资源查找、名称匹配和加载失败；每个 `Scene` 包含脚本和可选的本地化表。场景调用由解释器保存返回位置。

CST 的姓名／正文事件同时提供 CP932 基础文本和可选的本地化文本，保留空本地化值。正文缺失语言时的最终回退策略交给调用者。直接编号的基础文本字符串替换已执行，本地化文本中的额外替换和控制标记处理仍需接入消息宿主。

CST、FES 的事件在确认前会重复返回。CST 用 `resume` 确认普通事件，选项用 `choose` 按列表位置选择；结果 ID 可以重复。`choose` 转移到该项目标场景，系统变量和界面返回协议仍由后续宿主集成处理。FES 用 `resume` 确认事件，`Frame` 需要等到下一逻辑帧再继续。`Exit(code)` 是退出通知，确认后继续下一语句；接收任务属性的上层决定后续生命周期，FES 不因此返回 `Halted`。

FES 内部执行宏声明，初始化按以下顺序交给宿主：

1. `Declarations` 报告 INCLUDE、PRELOAD 和 KEYBLOCK。
2. 逐条 `CreateObject(ObjectDefinition)` 请求创建 DEFINE 对象。
3. 逐条 `InitializeObject(Box<ObjectInitialization>)` 请求应用 OBJECT 属性，每段使用自己的列定义。
4. 注册子命令表，`Declarations` 报告 SUBCOMMAND。
5. 开始 START 主流程。

`ObjectDefinition.kind` 为基线类型表中的大写名称，`name` 为对象名，`banks` 为有效银行数。`ObjectInitialization` 包含目标引用和已求值的属性；`Option` 区分未指定字段与明确写零，POS 的三个分量分别保留。POS2／SIZE2 保存在 `position_resource`／`size_resource` 中，由宿主读取所指定图像帧的裁剪偏移／裁剪尺寸。`runtime::fes::Objects` 提供 BASE／CIMAGE／IMAGE／CPLANE／PLANE 的身份、银行绑定、父子关系及属性应用；两类平面、CIMAGE 及 IMAGE 普通文件来源的下层媒体状态可显式更新。没有实际属性的行不发出请求。

`fes::ImageReference::parse` 将 POS2／SIZE2 的值拆为资源名称字节和帧 ID。省略帧号时取 0，冒号后读取带可选符号的十进制／十六进制整数前缀，非法前缀取零，不运行表达式。

`hg3::Metadata::parse` 读取 HG-3 0x300 的帧顺序及 stdinfo，保留裁剪尺寸、偏移、完整画布尺寸和绘制基点。`frame(id)` 返回第一个匹配 ID 的帧；ID 与数组下标分别保存。`StandardInfo::reserved` 保存 stdinfo 的 Alpha 标记，零表示不透明，非零表示使用 Alpha。该接口检查帧及标签范围，不解码像素。

`hg3::decode_frame(bytes, id)` 解码首个匹配 ID 的原生 32 位完整帧，缺失 ID 返回 `None`。`DecodedFrame` 包含 `id`、标准元数据 `info` 和从上到下紧密排列的 BGRA8 `pixels`，Alpha 保持独立，不应用裁剪偏移或绘制基点。支持范围与约束见 [图像接口](graphics.md)。

`fes::CImageState` 保存 CIMAGE 的坐标、尺寸、颜色、平面引用和显示属性。宿主按具体银行调用 `apply(&attributes, bank)`；返回本次是否使显示属性失效，供宿主递归标记后代。此接口仅应用该类型的有效列，不用于 IMAGE 等其他对象。父子对象关系应由宿主先处理，资源查找和绘制也由宿主负责。

`updates: ImageUpdates` 分别保存属性、来源和排序更新，构造时均为 false，可通过 `is_dirty()`、`needs_rebuild()`、`priority_changed()` 查询。尺寸实际变化才请求重建；颜色每次写入均请求重建，包括相同颜色。POS2 每次写入使属性失效；SIZE2 只保存引用，后续来源准备时读取。PRI 只请求排序更新。直接修改公开显示字段后调用 `updates.invalidate()`，修改颜色后调用 `updates.request_rebuild()`；后代失效由对象存储维护。

尺寸通过 `set_size([width, height])` 修改，同时维护导航偏移缓存及来源重建标记。`set_key_position(mode, [x, y])` 对应对象命令 `keypos`，保存原始模式与偏移，不请求媒体更新；零模式累加和相同尺寸重设的规则见[对象控制命令](graphics.md#fes-对象控制命令)。直接改写 `size` 会绕过缓存维护。

`base_point`、`scale`、`rotation` 保存运动可修改的二维基点、缩放及弧度旋转。`subcommand_context()` 提供调用换算所需的当前状态；其中 base 的 Z 分量使用对象优先级。`motion_properties()` 生成运动传递区，`apply_motion_properties(&properties)` 回写位置、基点、缩放、旋转、Alpha、优先级和 display。IMAGE 使用 `base` 上的相同接口。每次新运动更新前，把当前属性交给 `MotionTask::set_properties`；更新完成后读取 `state().properties` 并回写。预算续跑期间维持同一份输入。

`resolve_geometry` 接受提供 `StandardInfo` 的加载闭包，在对象更新阶段先应用 SIZE2 的裁剪尺寸，再应用 POS2 的裁剪偏移。SIZE2 引用持续保留，POS2 在尝试读取时消耗；加载错误直接返回，此前已完成的状态修改保留。原引擎在读取失败时使用零坐标／尺寸，本库保留诊断。

坐标以 f32 保存；一次 POS 更新会将未指定的已有分量也转为整数再写回。`priority` 为绘制优先级：原设置函数在输入与当前值不等时保存输入加 20，相等时保留；`parent_priority` 保存本次 PRI 的输入值，实际用作共享图像列表的槽号。数组递增在这一步之前完成。运动回写保持槽号，只更新绘制优先级及相应标记；`apply_motion_properties` 也返回本次属性失效状态。

`fes::ImageState` 为 IMAGE 保存基础显示状态、资源名、当前图像编号和 32 项帧映射。`apply` 接受按配置转换资源名称的闭包，成功返回本次显示属性是否失效；空 FILE 只清空请求名称，保留当前名称。`refresh_file(transform_name)` 根据保留的请求重新计算当前名称，变化时请求重建；空请求保留旧来源。此阶段保存状态，实际资源加载和像素绘制由宿主在对象更新阶段完成。IMAGE 不应用 CIMAGE 的 SIZE／COLOR 列。

`selected_frame()` 返回当前映射后的帧 ID。初始映射未启用时，IDN 可直接指定任意帧 ID；设置任意 ID 列后启用映射，后续 IDN 仅接受 0～31。一次初始化中先应用 IDN，再更新 ID 映射，因此首次启用映射时仍可能保留范围外的当前编号，此时 `selected_frame()` 返回 0。POS2 由对象属性登记阶段消费，IMAGE 普通文件更新不读取 SIZE2。

IMAGE 通过 `base.updates.needs_rebuild()` 保存来源装载请求。FILE 的转换结果实际改变时置位；空 FILE 或转换结果相同不置位。`frame_mapping_changed` 初始为 true，写入任何 ID 列也置为 true；下次有效 IDN 在编号变化或此标记为 true 时请求装载并清除此标记。首次 IDN=0 因此也请求装载。ID 列本身只更新映射，非法 IDN 保留待应用标记。直接修改公开映射后应同时设置 `frame_mapping_changed`；实际来源装载由运行时对象更新执行。

`fes::ButtonState` 组合 IMAGE 状态与输入启用、默认事件组、四向目标、TYPE、KEYBLOCK 坐标和系统键。`apply(attributes, bank, transform_name)` 返回本次是否需要递归使后代失效；SIZE 变化另请求来源重建，ENABLE 保留原始值，变化时只标记按钮自身。TYPE 与 MASK 分别保存。默认事件组和方向目标各少于 32 字节。运行时已支持 BUTTON 装载创建、图像更新、输入区域、对象事件、网格导航、系统键分派与关联 SLIDER／SCROLL2 的方向调整。 `position_lock` 保存原始坐标锁值，`locked_position` 保存首次非零锁定时的浮点位置；通过运行时对象命令 `lock` 更新，直接修改字段时调用方负责维护捕获状态。坐标、区域重建和导航规则见[BUTTON 区域更新](graphics.md#fes-button-区域更新)。

`fes::SliderState` 提供 SLIDER 的初始化属性、原始范围／数值、矩形、方向、Alpha 和独立的 `SliderUpdates` 请求。`apply(attributes, bank)` 仅应用 FILE、PL、POS2、PRI、DISP、ENABLE，返回本次是否需要使后代失效；ENABLE 变化只标记自身。`set_range`／`set_position` 保留原始值和通知参数，不执行范围限制。运动接口只传递 Alpha，详见[SLIDER 属性核心](graphics.md#fes-slider-属性核心)。运行时已连接下层任务、媒体更新和父级通知。`fes::Scroll2State` 组合此状态与独立页长，FES 媒体通过类型 41 的对应命令提交，`input::Scroll2Object` 负责状态图像、输入及生命周期。

`input::SliderControl` 提供下层滑块独立的范围、行程、数值、像素位置与拖动状态。`set_range` 只保存端点，`set_position` 先限制数值，非拖动状态再更新像素；`update_pixel_position` 可显式刷新。`position_at_pixel` 使用基线的半步阈值返回从零起算的数值，除法异常及过大查询返回诊断。FES 原始请求与这一状态分层保存，换算规则见 [SLIDER 下层范围换算](graphics.md#slider-下层范围换算)。

`input::Scroll2Control` 保存滚动条范围、页长、行程、轨道及滑块长度和拖动状态。默认范围为 `[0, 10]`，页长为 4。修改范围或尺寸后用 `rebuild_geometry()` 重建滑块和行程，再通过 `set_position`／`update_pixel_position` 刷新位置；几何重建本身保留数值和像素位置。`position_at_pixel` 查询数值，零有效跨度返回零。页长、最小长度与整数规则见 [SCROLL2 下层范围及几何](graphics.md#scroll2-下层范围及几何)。

`Scroll2Control::compose_images(frames, vertical)` 使用当前 `knob_extent` 和 `minimum_extent`，从已解码帧表生成四份独立状态图像；false 为横向，true 为纵向。调用方先设置这两个长度，方法不重建数值几何、不读取资源，也不修改来源。帧分组、偏移与缩放规则见 [SCROLL2 状态图像](graphics.md#scroll2-状态图像)。`Scroll2Object` 已复用此入口持有状态图像并连接列表和输入。

`input::Scroll2Object::new(id)` 创建未装载的类型 41 任务；`values()` 查询下层数值与几何，`buttons()` 按轨道／滑块顺序返回可选任务编号，`direction()`、`current_state()`、`loaded()` 和 `alpha()` 查询实际状态。与运行时组合时由 InputFactory 创建，独立宿主须提供资源、图像存储和任务路由。命令及释放契约见 [SCROLL2 下层任务](graphics.md#scroll2-下层任务协议)。

宿主完成请求后用 `resume` 确认，待确认请求重复返回，数量和属性表达式只计算一次。字符串表必须在初始化前准备好，缺失字符串可能使后续参数移动。宿主只应在具备对应实现时确认声明或属性请求。`return_value` 保存最近一次 `break` 的值；不带参数的 `break` 将它重置为 0。

`Interpreter::object_reference(input, state)` 解析对象名及银行表达式。名称精确匹配；省略银行号或值为 −1 时，已知多银行对象保留 −1 表示整个数组，其他对象归一为 0。其他引用的查找和银行范围由宿主处理。KEY_* 中名称为 `0` 的引用解码为空名称和银行 0，表示解绑。对整个数组指定 PRI 时，`sequential_priority` 为真，宿主应按银行号递增优先级。

`fes::parameters` 仅按 FES 语法提取字节词项；`Interpreter::parameters(input, state)` 还展开词项中的字符串变量。宿主用后者读取 `Command` 提供的已展开行，避免重复进行宏替换。Tab、圆括号和双引号的具体边界见 [FES](fes.md)。

宿主处理初始化声明时，可调用一次 `Interpreter::keyblocks(state)` 解码焦点网格。返回 `Vec<KeyBlock>`，`size` 为横向／纵向按钮排列数，`neighbors` 按上、下、左、右保存离开网格后的目标块，−1 表示没有目标。该入口只读取第一段 KEYBLOCK；数量按整数前缀读取，零或负数得到空表。未写条目的尺寸为零、方向为 −1，重复行保留未给出的方向分量，越界编号的行跳过。按钮与网格位置的绑定以及实际键盘／手柄导航由宿主负责。

`Interpreter::subcommands(state)` 返回 `SubcommandTable`，包含按命令名字节索引的绑定和逐行诊断。同名命令保留第一次定义，多个 SUBCOMMAND 段按顺序处理。每个绑定保存类别、原运动资源名、参数描述符及默认文本，以及 `MappedArgument` 映射。`Interpreter::subcommand_arguments(binding, arguments, context, state)` 将去除对象名／命令名的已展开参数换算为运动参数数组。`SubcommandContext` 由宿主按目标银行提供基点、位置、速度、弧度旋转、缩放和 ARGB 颜色；求值使用当前 FES 的局部与命名数值，`@` 为当前属性的整数值。参数编号越界返回诊断。低层解释器保留该宿主边界，Runtime 的 Scene 已提供 BASE、SLIDER 和三类图像对象的运动装载及调度；SLIDER 使用默认上下文，Alpha 仅在运动属性块中传递。

`run` 自动将已注册子命令发为 `Event::Subcommand(Box<SubcommandCall>)`。调用包含源码字节位置、对象与银行、绑定及已展开输入；`source` 保留行级展开后的完整命令。银行 −1 表示整个数组，事件内的 `@` 对象引用解析为触发对象与银行。宿主逐个查找目标银行，提供各自的 `SubcommandContext`，调用 `subcommand_arguments` 后创建并插入运动任务，再用 `resume` 继续 FES。Runtime 首次使用绑定时调用资源服务的 mot 类别补全，并在场景内保留名称；场景存活期间应保持类别映射稳定。成功读取的运动程序在 Runtime 中按忽略大小写的完整名称缓存，实例拥有独立 VM 数据。资源缺失时跳过创建并继续脚本，读取错误保留当前银行的换算参数。基线两类绑定均以分组 0 入队。起始目标缺失时，Runtime 在资源补全与参数映射前对 source 执行整行回退。

`registered_subcommands()` 在声明事件发出后返回已注册表；绑定名称与目标保留注册时的字符串展开结果。125 个原生命令优先于同名绑定，名称区分大小写；其他命令继续通过 `Command` 交给宿主。待处理调用重复返回时，不重新执行银行表达式。

`SubcommandParameter.descriptor` 是基线表中区分大小写的名称；省略默认值时保存 `-1`，读取阶段不求值。`MappedArgument::Parameter` 保存映射编号，`Integer` 保存整数前缀，`Context` 保存 cxr／cxg／cxb／cxa／CA 上下文标记。描述参数和映射参数各最多 16 项，默认文本最多 15 字节；越界映射编号需要在实际调用时根据参数表检查。非法行返回带源位置的诊断，其余绑定继续读取。

对象宿主调用 FES `dispatch_event(object, bank, event, default_handler)`，提供对象名称、银行号、事件名称及可选默认事件组。当前支持 `FOCUS`、`UNFOCUS`、`PUSH_L`、`PUSH_R`、`RELEASE_L`、`RELEASE_R`、`SCRL_P`、`TOUCH` 和 `PLAY`，返回是否存在处理器。PLAY 的非空默认事件组按媒体别名路径返回诊断。初始化完成后，允许在 `Wait`、`Frame` 或尚无待确认操作时分派；创建、宿主命令、声明尚未完成或另一个事件正在执行时返回错误。

分派后继续调用 `run`，命令通过 `Command` 或已注册绑定的 `Subcommand` 事件交给宿主，`event_context()` 提供对象与银行号，供宿主解析 `@` 对象命令；数值表达式中的 `@` 已使用银行号。事件完成时 `run` 返回 `Yield`，上下文变为 `None`；下一次 `run` 恢复主流程原等待，或执行事件交出的目标位置。两类事件中的 `next`／`call` 差异详见 [FES](fes.md)。事件内收到 `Wait`／`Exit` 后，宿主调用 `finish_event_handler()` 结束当前处理器；默认组结束后仍会进入对象自身处理器。`Exit` 的通知码由宿主另行保存，主流程等待及尚未返回的调用保持。嵌套分派返回错误。

`anm::Timeline::new(program)` 提供时钟调度。设置 `speed`／`origin` 后调用 `update(now, budget)`，返回 true 表示本轮完成，false 表示预算让出；续跑保留本轮时间。`interpreter.display` 为最终显示状态，`display_changed()` 为本轮是否改变，`is_finished()` 为持续时间和全部指令是否结束。`update_with_random` 接受共享随机回调。`update_frame`／`update_frame_with_random` 另处理对象的零起点和 `u32::MAX` 时间哨兵。`restart()` 重置执行位置、持续时间和显示，保留变量、最大帧 ID、速度及时间基点。`DisplayState::compose` 组合图像自身坐标、混合值和显隐，保留帧 ID。播放开关由宿主控制是否更新，恢复后不平移时间基点。原始指令入口仍可分别使用：ANM 的 Image／Wait 事件携带持续时间。宿主完成该时长后再调用 `run`；库当前不绑定刷新率或墙上时钟。零耗时跳转循环受指令预算限制。随机区间通过 `run_with_random` 从宿主读取 u32 随机数，固定区间不消耗随机数。`display` 保留当前显示属性，`max_image_id` 由图像宿主设置。

`anm::ImageAnimation` 保存图像持有的可选时间线和显示缓存。`load(bytes, max_image_id)` 先移除旧时间线，成功后初始化全新运行状态，失败则保持无动画；对象的播放开关和首次更新时间标记保持。`update(now, budget, has_reset_frame)`／`update_with_random` 完成一轮后，通过 `frame_change()` 提供可选选帧请求，`display()` 提供缓存；未绑定动画时使用图像自身属性。资源读取可由 `ResourceRouter` 提供，图像宿主负责最大帧 ID、保留帧 10000 是否存在及实际选帧。完整装载与续跑契约见 [ANM 文档](anm.md)。

`ImageAnimation::restart(budget, has_reset_frame)`／`restart_with_random` 重启现有时间线，并强制按哨兵时刻更新一次，保留原播放开关。返回 false 时通过普通更新接口续跑，保留该次强制更新的时间和选帧条件；`Timeline::restart()` 本身不触发推进。

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

执行器以字节地址管理程序数据、栈和最高位标记的外部存储；全部访问均检查范围。`Program::instructions` 提供只读的解码指令。`Interpreter::debug()` 提供当前调试位置、作用域地址和变量索引。通用树更新／销毁已由 `TaskTree` 与 `Lifecycle` 提供，完整对象工厂和游戏主循环尚未整合。

`kcs::CoreHost::new(&mut state)` 在调用者已初始化的 `expression::State` 上提供普通任务表 native 0～6、8、13～26、33～38、40～42、44、45、47、48、50～65、68、71、75／76、149、209～213、276～278、319、393、419～423、465、515～519、741、789／790、885／886、894～899、902、999。共享数值区、字符串表和当前参数值可与 CST／FES 复用；209 保留已经存在的状态。789 返回 `configured_shared_slots`，初值为传入状态的槽数，可根据独立配置设置。50～64 提供普通表标量数学，62／63 保留分步角度换算，65 让出本轮且不压值。319 执行先加 0.5 的取整；5 返回 `task_time − task_origin` 的 u32 回绕差，两字段由任务宿主提供，初值为零。其他编号返回明确错误，自定义宿主可按编号委托给 `CoreHost`，其余接口自行实现。运动和粒子程序的编号属于其他函数表，调用者必须提供相应宿主。持久字段保存在 `kcs::HostState`，`CoreHost` 通过 `Deref` 暴露同一组字段；`HostState` 也可独立持有，在多次 native 调用之间保留集合句柄、随机序列和请求队列。`&mut H` 同样实现 `Host`，动态宿主可以直接传给要求具体宿主的入口。

`kcs::TaskClock::update(now)` 接受外层调度器提供的整数时刻。`paused` 非零时平移起点并保留当前时刻，清除 `one_tick_pending`；未暂停时更新当前时刻，有单步标记时还将“与上次更新的差值减一”加到起点，并立即清除标记。零时间差同样消耗标记，全部差值与起点运算按 u32 回绕。`elapsed()` 查询当前差值，`CoreHost::set_task_clock(&clock)` 将其同步到 native 5。子任务遍历与等待调度由外层负责。

`HandleAllocator::default()` 创建从一开始的编号源，克隆后共享分配进度；`allocate()` 返回非零不透明编号，耗尽时返回诊断且不回绕。释放对象不归还编号。`HostState::with_handles(handles)` 和 `graphics::Images::with_handles(handles)` 可将独立存储接到同一编号源，类型及生命周期继续由存储检查。`Host::handle_allocator()` 默认返回 None；CoreHost 和 Runtime 的对象宿主提供当前编号源，TaskHost／SystemHost／`&mut H` 转发，ResourceHost 优先沿用内层编号源并向外提供它。自定义的句柄宿主应提供同一服务；没有内层编号源的 ResourceHost 自建独立源。句柄不是 VM 地址，也不是任务编号。整体替换 `CoreHost::host` 或 `Runtime::host_mut()` 返回的状态时，使用 `HostState::with_handles(old.handle_allocator())` 沿用原源，避免与仍存活的外部资源冲突。

`Interpreter::execution_mode()` 初值为 1，`set_execution_mode(mode)` 保存原始 DWORD 并清除 `debug().step_over_return`。模式 0 时普通 `resume` 返回 `Step::Paused`，不执行指令、不消耗等待状态；这表示本次调用完成。低层 `run` 和消息分派绕过这一暂停。模式 3 在执行代码调用指令时改为 1，并把该调用的返回位置保存在 `step_over_return`；其他非零模式沿用普通执行。

普通任务使用 `Interpreter::resume(host, budget)` 进行无消息更新；native 71 登记接收地址并让出，下一次普通更新补入 0。`dispatch_message(&mut [u32; 4], host, budget)` 在等待时复制消息、补入 1 并执行，完成后回写消息第二项；未等待时返回 `None`。`Some(Step::Yield)` 表示预算续跑，继续传入同一消息，其间调用 `resume` 返回诊断。`is_waiting_for_message()` 查询等待状态；自定义宿主可调用 `wait_for_message(address)` 注册接收地址。运动／粒子的低层 `run` 不消费消息等待。接收地址切换及回写规则见 [KCS 宿主契约](kcs.md)。

`kcs::OrdinaryTask::new(program, external)` 创建已装载且 running 为 1 的任务；`clock`、`control`、`creation_context`、`interpreter`、`task_id` 和 `running` 可由外层管理，任务编号默认零，创建上下文默认空。`enqueue_message([u32; 4])` 复制入队，`queued_messages()` 查询当前队列。`update(now, increment, host, budget_per_call)` 更新时钟、按顺序分派全部消息并进行一次无消息恢复，返回 true 表示本轮完成；false 时继续调用同一方法，本轮时刻和累计增量保持不变。预算分别交给每次 VM 推进，`is_pending()` 查询续跑状态，续跑期间禁止入队、切换执行入口或直接操作 VM／控制字段。running 为零时只更新时钟并保留队列；时钟 paused 非零仍推进活动脚本。`accumulated_increment` 仅在活动的新一轮累加外层提供的 increment，按 u32 回绕。程序在执行中到达隐式代码末尾时保留 running 为 1，下一轮进入才停止；显式停止指令当轮停止。

`OrdinaryTask::initialize(host, budget)` 提供装载后的单次普通恢复，不更新时间、不消费队列、不累计 increment；false 表示预算续跑。`OrdinaryObject.task` 为 `Option<OrdinaryTask>`，无 VM 时为 None；已有调用方需先借用该选项或通过 `TaskObject::interpreter` 查询。

`OrdinaryTask::update_body(increment, host, budget_per_call)` 执行消息队列及 VM 主体，省略时钟更新。树调度器先更新该任务的 `clock`，再推进子任务，最后调用本方法；子任务期间可以改变父任务控制字段或入队。主体预算续跑期间继续调用本方法，禁止再次更新时钟或重复运行子任务。原有 `update` 继续提供单任务的时钟与主体组合。

`OrdinaryTask::start_execution(source, mode)` 对应普通命令 162：将 running 设为 1；source 等于当前 task_id 时使用传入 mode，否则使用 1。source 对应命令第一数据项。`stop_execution()` 对应 163，将 running 和 VM 执行模式均设为零。两方法在已有删除标记时跳过，预算续跑期间返回诊断。VM 暂停仍允许时钟、累计增量和排队消息处理，末尾普通恢复完成后保留 running 为 1。

`OrdinaryTask::dispatch_control(kind, command, output, host, budget)` 接收基类控制、25／26、162／163，以及 4／9／10／18／20／21／22／30／31／64／65／452 的消息通知。25 把创建上下文正文和一个结尾零写到第二数据项指向的地址，26 从该地址读取零结尾正文；零指针跳过。kind 来自任务树节点；返回 `task::CommandOutcome`：`done` 为 false 表示通知仍需以同一命令及 output 续跑，`propagate` 表示是否允许按 `data[2]` 继续传播。已处理的普通命令和已有删除标记的对象返回 `propagate = false`；其他编号交给基类，尚未实现的已知专属编号返回诊断。同步通知不受 running 限制，20／21 复制入队；消息映射、返回槽和预算规则见 [KCS 普通任务](kcs.md#普通任务与跨任务宿主)。

`set_creation_context(Some(bytes))` 对应命令 26，保存首个零之前的原始字节，正文最多 31 字节；`copy_creation_context(Some(buffer))` 对应 25，写入正文及一个结尾零，保留输出其余字节。两方法的 None 均跳过，删除后的任务也跳过；预算续跑期间返回诊断。输入或输出容量不足时保留原状态，直接使用公开上下文字段时由调用者保持正文容量约束。

`OrdinaryTask::dispatch_message(message, output, host, budget)` 同步投递消息，完成后把活动结果写入 output；未等待时写零，已有删除标记时保留 output。投递不受 running 限制，也不修改时钟、running 或累计增量。`request_delete(host, budget)` 先向等待中的 VM 同步投递内部删除通知，再把 `control.deletion_requested` 设为 1。两方法均以 false 表示预算续跑，只能继续同一入口；消息分派需继续传入同一消息和 output。删除后的更新只更新时间，外层负责移除任务及回收子树。

普通任务在更新、同步消息及删除通知前通过 `Host::set_task_clock`、`set_task_id` 和 `set_task_context` 同步当前时钟、编号及原始创建上下文；包装宿主应委托这三个方法。`Host::take_task_effects(queued_messages)` 接收当前队列长度并返回 `TaskEffects { delete, messages }`，默认无操作；普通任务在每次 VM 推进前后应用它，保留处理消息期间继续入队的时点及容量检查。包装宿主同样需要委托此方法。`CoreHost` 已接入时钟及编号，`TaskHost` 保存上下文供创建使用。4 返回外层平台计时器提供的 `CoreHost.system_time_ms`，初值零，单位毫秒，按 u32 回绕；该字段独立于任务时钟更新。任务树、创建／删除和对象命令由外层负责。

`CoreHost.pointer_position: [i32; 2]` 提供普通表 149 的当前逻辑指针坐标，初值 `[0, 0]`。独立宿主在查询前更新该值；Runtime 绑定 InputDevice 后在每次 149 调用时即时查询并经共享 CoordinateMap 换算，无设备则使用既有缓存。负数及画布外坐标保留。149 依次写 X 和 Y，不压入返回值。任务编号和时钟同步不改变坐标。

### 同步系统服务

`kcs::ResourceHost::new(&mut inner, &mut services)` 提供普通表 590～593 的 XML 装载、释放、整数及文本查询。服务实现 `ResourceServices::read(name, mode) -> Result<Option<Vec<u8>>>`；输入为格式展开的字节名称，当前 XML 接口采用 `ResourceMode::LooseFirst`。可由 `ResourceRouter::read` 转交已注册的归档和文件读取，再取返回资源的 bytes。None 或解析失败使 590 返回零，服务错误停止当前推进；创建的句柄存活至显式释放或宿主销毁。零句柄查询跳过地址解析，非零失效句柄返回诊断。593 的目标需为有效零结尾串，缺失字段保留原值，成功时仅覆盖 UTF-8 字段及结尾零。

594～597 提供独立的三整数分组表：594 以格式化资源名称装载，595 释放，596 按组名、597 按条目名查询。596／597 接受句柄、格式化名称地址及三个输出地址，找到时依序写入原始 DWORD 并返回一；未找到或零句柄返回零，保持输出。读取同样采用 `LooseFirst`；成功打开的空文件仍返回有效空表句柄。分组、注释、首次定义及不完整头部的规则见[资源格式](resources.md#三整数分组表)。

同一 `ResourceHost` 提供 365／366 的图像原始数据装载及释放。服务需实现 `resolve(category, name)`，以 `ArchiveConfig::resolve` 等价规则补全 image 类别，随后 `read` 按散装优先读取完整字节。空资源或 None 返回零，其他成功数据创建独立句柄；`image_data(handle)` 可借用其内容，366 或宿主销毁结束生命周期。解析 XML 与加载图像使用不同类型的句柄，错误类型和失效句柄返回诊断。图像解码及绘制对象尚未通过这组接口创建。

368 可从已装载的 HG-3 0x300 数据读取帧元数据，向目标写九个 DWORD；字段顺序见 [KCS 文档](kcs.md)。它返回找到状态，零句柄不写目标，正常查询先清零目标字段。目标需能容纳 36 字节；HG-2／JPEG 查询及未支持的 HG-3 版本返回诊断。

`kcs::SystemHost::new(&mut inner, &mut services)` 为已有 Host 接入普通表 629。服务实现 `SystemServices::open(task_id, target, parameters, directory) -> Result<()>`，接收依次格式展开后的三个字节串。服务返回后 native 才以状态 2 继续，不压值；服务错误返回解释器诊断，当前推进立即结束。平台打开 API 的成功／失败返回值均不作为 VM 返回值。

包装层把时钟、编号、创建上下文及其他 native 委托给 inner，可与 TaskHost 组合。初始编号为零，直接使用 VM 时通过 `Host::set_task_id` 设置；OrdinaryTask 自动同步。平台服务按编号查找相应窗口或环境，并负责处理字节路径、参数和目录，具体 Windows 调用契约见 [KCS](kcs.md)。

### 任务编号与父子索引

`task::TaskTree::new(root_id, kind)` 登记根任务。`insert(parent, id, kind)` 登记已有对象的编号，`insert_auto(parent, kind)` 选择整棵树中最小可用的非负编号；两者只登记索引，调用者负责实际对象及资源的创建。编号全树唯一，父级必须存在。`node(id)` 读取编号、父级及类型，`children(id)` 按 i32 编号升序枚举仍登记的直属子任务，`descendants(id)` 以相同顺序枚举该层可查询的后代；`find_in_subtree(scope, id)` 只查给定任务自身及其后代。

`unregister(id)` 只撤销当前编号在祖先索引中的值，保留存活节点、父指针和后代编号；根节点不能撤销注册。`node(id)` 仍可取得对象元数据，祖先的 `find_in_subtree` 随即跳过该编号，但继续查询其后代；任务查询自身仍成功。这一状态供同步删除通知及递归析构使用。

`subtree_ids(id)` 返回父任务优先、同级按有符号编号升序排列的编号。其中包含已撤销注册的存活节点。对象销毁时调用 `remove(id)` 清除整个子树，返回相同顺序的编号，供外层释放对应执行状态；根节点不能单独移除。销毁的编号继续保留到 `sweep()`，该方法应在根任务完成索引清理时调用，随后自动编号才能复用空位。`next_id()` 只查询下一编号。直接指定编号的 `insert` 可以重新登记已经销毁的空位。

普通宿主通过 `CoreHost.task_tree = Some(&tree)` 提供查询上下文。75 接受从零开始的后代索引序号，越界返回 `u32::MAX`；76 无参数，返回当前任务父级编号。没有任务树、当前编号未登记，以及对根任务调用 76 均返回诊断。树的更新、销毁时点和对象命令仍由外层负责。

### 任务命令路由

`task::Command` 保存 `target`、`sender`、`code` 和三个原始 DWORD `data`。`TaskTree::dispatch(origin, command, &mut output, handler)` 从存活的 origin 发送命令；`sender` 可以不同于 origin。路由先查 origin 自身的子树索引，再沿父指针查找；可达目标返回 true，缺失返回 false 并保留 output。已撤销注册的祖先仍可通过父指针收到来自后代的命令。处理器签名为 `FnMut(&mut TaskTree, &Command, &mut u32) -> Result<bool>`，接收实际目标编号；它负责对象自身的阻止分派状态、通用／专属命令及输出写入。返回 false 终止当前分支，返回 true 且 `data[2]` 非零时继续向直属子任务传播，其他兄弟分支仍执行。

`broadcast(origin, group, command, &mut output, handler)` 对应外层任务命令 11／12，传入的 `command.code` 为内层命令号。`group=None` 选择全部类型，`Some(kind)` 按节点类型筛选。路由先在 origin 及向上经过的非根任务索引中查找目标，命中时从目标路径上的首个直属子任务开始；未命中则转给根层并从整棵树开始。origin 就是根任务时，先查询根的子树索引。每个接收者的目标编号被替换，sender 保持传入值，`data[2]` 固定为零；处理器返回 false 不阻止广播遍历其后代。

广播返回最后遍历叶节点的处理结果；类型未匹配的节点结果为 true，output 则保留处理器实际写入的值。这个布尔结果与 output 分别使用。直接分派和广播均按父任务优先、同级 i32 编号升序推进。处理器可通过传入的树登记或移除节点：当前游标之后新增的节点参与本次遍历，之前的编号留到下次，已销毁子树跳过。

创建／登记命令 1／2 由对象工厂和索引接口处理，广播 11／12 使用专用方法；把这些编号当作内层命令传入会返回诊断。每次遍历最多访问一百万个节点，防止处理器持续创建节点导致不结束。

### 任务基类控制

`task::Control` 保存 `deletion_requested` 和 `property`，初值分别为零及 `u32::MAX`。`dispatch(kind, &mut clock, &command, &mut output, specific)` 可直接放入任务树的 handler：删除标记非零时返回 false，跳过 specific；否则先调用专属处理器，它返回 true 时再执行基类规则。specific 接收 `(&mut Control, &mut TaskClock, &Command, &mut u32)`，可修改本对象状态。删除标记仅在调用 specific 前检查一次。

|命令|基类行为|
|--:|:--|
|3|将删除标记置 1，等待外层实际销毁|
|13|将传入的对象类型 kind 写入 output|
|17|消费命令，保留状态和 output|
|23／24|查询 property／将 `data[1]` 原样保存到 property|
|29|将 `data[1]` 的 i32 位模式保存到 clock.paused|

上述命令均返回 false，消费当前分支。其他未识别编号返回 true，允许调用链继续；登记 2 和序列化 15／16 若到达基类则返回未实现诊断。对象专属处理器可以先消费这些命令。删除标记不会自动从树中移除节点；外层在销毁时调用 `remove`，再按根层清理时点调用 `sweep`。

`task::TaskClock` 与 `kcs::TaskClock` 是同一类型的两个导出。普通脚本使用自己的 `OrdinaryTask.clock` 传给 Control，暂停命令的效果随下一次任务更新生效，暂停期间脚本仍按普通更新契约执行。

### 任务树生命周期

`TaskTree::update(now, increment, &mut lifecycle)` 完成整棵树的一轮同步更新：先更新父任务时钟，再按有符号编号推进直属子任务，清理根层编号占用，最后检查删除标记并执行对象主体。返回 true 表示根仍存活，false 表示根已经关闭。`destroy(id, &mut lifecycle)` 直接完成该子树的同步销毁，也可关闭根任务；根关闭后 `node(root_id)` 返回 None，再次 update 返回诊断。

`task::Lifecycle` 由对象服务实现，方法契约为：

|方法|职责|
|:--|:--|
|`state(id)`|返回该对象的 `(&mut Control, &mut TaskClock)`，供入口更新时间及子任务结束后检查删除标记|
|`update_body(tree, id, increment)`|执行对象主体，返回前完成所有预算续跑；普通任务委托 `OrdinaryTask::update_body`|
|`request_delete(tree, command)`|完成子任务删除命令 3 的专属通知及标记；sender 是正在销毁的父任务，data 全零|
|`mark_destroying(tree, id)`|子任务请求阶段结束后处理自身通知及标记；普通对象可调用自己的 `request_delete`，基类对象直接设置 Control|
|`prepare_release(tree, id)`|子树已释放后执行需要宿主或树的资源清理；默认直接完成|
|`release(id)`|前置清理完成后释放当前对象自身资源和执行状态；返回后树移除当前节点|

销毁先撤销当前编号，再请求全部直属子任务删除，随后处理自身标记，最后按相同编号顺序逐个销毁子树，执行前置清理并释放当前对象。Runtime 将前置清理交给 `TaskObject::prepare_release(host, budget)`，可同步移除其他任务持有的登记；之后调用 `release()` 并回收所属图像等资源。回调可以使用传入的树查询当前可见性、登记对象或销毁其他分支；同步通知及预算续跑应在返回前结束。`unregister` 的中间状态由对应销毁路径完成，不应留到下一轮。执行中的新增节点按游标决定本轮或下轮参与；内部遍历使用显式栈及访问上限。

生命周期服务负责类型专属行为、资源和解释器所有权；树负责顺序及索引。具体规则和原入口对照见 [任务树与生命周期](tasks.md)。

### 任务消息与跨任务存储

`kcs::TaskHost::new(&mut inner, &mut services)` 包装现有 `Host`，接入普通表 66／67／69／70／72／73／108／109／110，其他 native 委托给 inner。包装时当前编号为零，直接使用 VM 时通过 `Host::set_task_id` 设置；`OrdinaryTask` 会自动同步编号、时钟和创建上下文，包装层继续委托给 inner。

`TaskServices` 要求宿主提供三个方法：`dispatch_message(target, &mut message)` 完成同步分派并保留消息回写，`enqueue_message(target, message)` 复制入队，`interpreter(target)` 返回目标任务的 VM。前两项返回任务命令处理器输出的 DWORD，而非消息正文；普通任务未等待时同步结果为零，排队结果为 1。同步回调必须在返回前完成本次分派的全部预算续跑，投递本身不修改目标的 running 或时钟。任务查找、沿树路由、对象类型差异及数值绑定切换由服务处理；缺失或不支持的目标应返回诊断。

服务可以通过 `task_tree()` 返回自己持有的树，`TaskHost` 随后从该树处理 75／76；缺省返回 `None` 时继续委托 inner。创建和查询因此可以共用服务内部的可变树，无需另外给 `CoreHost.task_tree` 保存长期借用，创建后查询立即读取更新后的索引。

66 接受对象类型、父任务编号和名称地址，先展开名称中的 KCS 格式引用，再调用 `create_task(sender, parent, kind, name, context)`，以状态 1 返回新编号；编号零有效。`TaskHost.creation_context` 提供当前任务的原始创建上下文，默认空，读取到首个零为止，不展开格式引用。创建服务负责实例化对象、登记父子关系，并按对象类型接收继承的上下文；原普通对象通过任务命令 26 保存它。该方法默认返回未实现诊断。

67 调用 `request_delete(sender, target)`，返回状态 2。服务应分派删除命令 3，让目标执行相应删除通知并标记待删除，在任务更新阶段完成实际销毁和索引清理。该方法默认返回未实现诊断。

`dispatch_command(command, &mut output)` 从 `command.sender` 同步路由，返回目标是否可达，目标处理器通过 output 写回结果。可直接组合 `TaskTree::dispatch` 与对象处理器；处理器消费命令时，路由仍返回 true。缺省服务返回未实现诊断。109 接受目标编号，发送命令 96，data 全零；以状态 1 返回可达目标的 output，未找到返回零。108／110 接受目标编号和值，分别发送命令 95／97，data 为 `[0, value, 0]`；忽略路由返回值和 output，以状态 2 继续。108 用于按钮响应回写，保留原始响应值。服务错误仍向调用者传播，三项均不向子任务传播。

69／70 均接受目标编号、消息类型和两个数据项，消息首项自动填入发送者编号。69 同步调用服务，执行中的自身任务直接返回零；70 调用排队服务，包括向自身排队。两者都把服务输出按状态 1 返回。

72／73 的参数均为目标编号、目标 VM 地址、字节数、当前 VM 地址。72 从目标读到当前 VM，73 从当前写入目标 VM，状态 2。目标为自身时使用同一 VM，`TaskHost.memory_copy_mode` 控制重叠复制，默认 Mmx，规则与普通表 35 相同；不同目标使用独立 VM 存储。地址可以属于程序数据、栈或外部区，全部访问仍检查边界。

33／34 从 `CoreHost.random` 的独立整数／浮点序列取值，调用者可赋予 `ScriptRandom::seeded(seed)`；默认种子 4357。36 按低八位填充给定字节数，37 写入 i32 的千位分组文本及双零，正号开关只对正数生效；最小整数的基线结果见 [KCS 宿主契约](kcs.md)。

465 返回 `CoreHost.debug_mode` 的原始 DWORD，初值零，不读取参数，以状态 1 压入返回值。调用者从全局启动状态提供该值，并为各任务维护一致的查询来源。它与 VM 的 execution_mode 分别管理；任务编号和时钟同步不会更新此字段。

419 创建空序列集合并以状态 1 返回非零句柄，不读取参数。420 释放给定句柄，421 清空并保留句柄，422 接受句柄和格式字符串地址并登记序列；三项均以状态 2 继续。423 接受句柄和格式字符串地址，以状态 1 返回是否匹配的 0／1。零句柄跳过文本地址解析，423 返回零；失效或类型不符的非零句柄返回诊断。集合按宿主实例持有，与整数字典及已接入的资源存储共用编号源，释放后不复用旧值。

序列文本按逗号拆分，合并空段，保留空白和大小写。前两段用下划线连接为键；只有一段时直接作为键，后续各段取首个有符号字节作为分量。登记为并集，查询要求对应路径和结束边存在；`0xFF` 与内部结束边共用 −1，精确行为见 [KCS 契约](kcs.md#序列集合)。文本展开后最多 254 字节、最多 64 个分量，超限返回诊断。458 已接入文件装载，后续分组访问和文件写出继续接入。

458 接受序列句柄和格式字符串地址，经 `config` 类别补全后以 `LooseFirst` 读取资源，以状态 2 继续。零句柄跳过名称解析及资源访问；空名称和 VM 地址零均按正常字符串处理。资源缺失或为空时保留旧数据，读取服务错误返回诊断。成功装载保留名称树；长度大于四且以 `CGL\0` 开头时移除四字节头并选择 256 字节记录跨度，否则保留全部字节并选择 64 字节跨度。421 只清空名称树，420 同时释放文件数据。

`HostState::sequence_data(handle)` 返回该集合的 `SequenceData` 借用，`bytes()` 为文件正文，`record_stride()` 为字符串记录跨度；失效或其他类型的句柄返回诊断。`ResourceHost` 与 `Runtime::set_resources` 使用同一装载契约。独立包装宿主需将 `Host::load_sequence_data` 转发给持有该集合的内层宿主；CoreHost 已提供存储。

`CoreHost.sequence_name_mode` 由配置宿主提供，默认零。登记时保留原键，并从键下标 2 起寻找首个前方为 `l`／`L` 的下划线：模式 0 另登记删除该字母的键，模式 1 另登记将字母替换为小写 `m` 的键，其他模式只保留原键。查询不生成变体，切换任务或时钟不修改该设置及既有集合。

393 接受一个 DWORD，按状态 2 输出异步菜单请求并继续 VM；`CoreHost.menu_item_requests` 按调用顺序保存 `MenuItemRequest { task_id, command_id: 0x77, value }`。窗口宿主在处理请求时按 value 的零／非零值禁用／启用菜单项，随后可清空队列。value 保留原始位模式，task_id 记录发送任务，适用于共享宿主切换。请求不压返回值、不暂停 VM；队列最多保留一百万项。

8 将 `CoreHost.surface_size` 的两个 DWORD 依次写入给定的宽／高地址，默认值为零，由渲染宿主更新。输出可以位于数据区或外部区，重叠时保留先宽后高的写入顺序。

35 复制给定字节数，`CoreHost.memory_copy_mode` 选择 `MemoryCopyMode::{Crt, Dword, Mmx}`，默认 Mmx。MMX 和 DWORD 分支保留向前分块读取／写入的重叠结果，其他长度使用 CRT 结果；具体块长选择见 [KCS 宿主契约](kcs.md)。

515 创建空整数字典并返回非零不透明句柄，516 释放，517 登记原始字节键及 DWORD，518 查询且在缺失时返回 `u32::MAX`。重复键保留首值，键不展开格式且区分大小写，零句柄跳过键访问。字典由同一 `CoreHost` 持有，释放或宿主销毁后失效；句柄不能当作 VM 地址，调用者应保持宿主覆盖整个字典使用期。

1 的调试文字保存在 `debug_messages`，调用者可读取并用 `std::mem::take` 取走；库保留字节编码。26 读取展开后的数值前缀，返回 f32 原始位模式；它与要求完整表达式的 213 使用不同契约。

`Interpreter::read_string` 有界读取零结尾字节串，`read`／`write` 用于任意字节。文本 native 保留原始编码，数值表达式需要 ASCII。字符串写入检查可用空间，参数字节数必须与接口契约一致。13／14 复制原始／格式展开文本，15／16 追加，18 读取展开后长度；这些写入保留双零结尾。24 以字符数量截断并返回字节数，40 检查基线双字节，47／48 读取原始／展开后字符数；25 的格式整数读取使用引擎前缀规则。19～22 的原始／展开比较具有不同参数顺序和字节符号规则，见 [KCS 宿主契约](kcs.md)。native 278 原地展开字符串；519 将给定参数写入目标，序号包括命令名且从零开始。

### 任务运行时

`runtime::Runtime` 组合任务树、对象存储、普通宿主状态和对象工厂，不再要求调用者自行实现 `TaskServices` 与 `Lifecycle`。`Runtime::new(root_id, root_kind, root, numbers, factory)` 接受已经创建的根对象；`update`／`destroy` 按树契约更新时钟、子任务和主体，`create`／`dispatch_command`／`send_message` 对应外层创建、命令和消息入口。`tree`、`object`、`host`、`numbers` 提供查询，`set_budget` 替换推进预算。

`set_resources(services)` 接收拥有的 `ResourceServices`，提供普通 111 的 image 类别补全及同步读取；`images()` 查询 `graphics::Images` 中存活的解码对象。111 按名称列表选择 HG-3 帧并保留首图几何，以状态 1 返回句柄；117 对非零句柄作底端起始的原始 ARGB 填充，以状态 2 继续。所属任务实际释放时解除其图像引用，额外使用者持有的图像继续存活，后续图像句柄不复用。名称重组、合成及读取限制见 [图像接口](graphics.md)。共享持有通过 `Images::retain`／`release_reference` 配对，缓存继续接入。

同一资源服务也处理 365／366／368 和 590～597，契约与 `ResourceHost` 一致。`image_data(handle)` 查询 365 装载的原始字节。365 装载的原始数据、XML 与三整数分组表句柄由运行时共享，存活至显式释放或运行时销毁；任务释放和 `set_resources` 替换服务均不回收它们，其他任务可以继续查询和关闭。解码图像与原始数据分别保存，不互换句柄。未配置服务时，装载返回诊断，零句柄查询／释放仍按各自规则执行。

解码图像、原始数据、XML、三整数分组表、419 序列集合与 515 整数字典共用一个运行时编号源；同时存活的不同类型对象不会获得相同编号，释放后也不复用。错误类型的读取／释放返回诊断，不释放该句柄真正指向的对象。任务回收和读取服务替换均保持编号递增；装载缺失、解析失败及尺寸检查失败不创建句柄。

图像包装与底层像素也分别分配句柄。`Images::pixel_handle(wrapper)` 只查询绑定关系，不增加引用；像素句柄在至少一个包装持有时有效，不能用于普通包装查询或释放。`ImageList::set_pixels` 按像素句柄登记并持有独立包装，`set` 接收普通包装后作同一登记。

`Images::recreate(wrapper, size)` 保留包装及任务登记，仅在宽高均变化时绑定新的全零像素；任一轴相同则保留旧尺寸及像素。其他包装继续持有旧像素，尺寸检查或分配失败保留原绑定。该路径与每次创建独立包装的普通 112 分开提供。

`graphics::ImageList` 接收普通图像包装并持有独立来源包装，或通过 `set_hg3` 独立持有全部 HG-3 帧。初始选择最小无符号 ID，重复 ID 取首帧；`select_frame` 恢复选中帧基点但不标脏，缺失返回 false。像素重登记保留帧缓存，并改绑当前帧包装；原始来源重登记替换缓存，解码错误保留旧来源，帧集合总像素上限为 64 MiB。列表另提供 `set`／`remove`／`set_visible`／`clear`、脏区 `update` 及显式全部面块 `redraw`；支持无变换图像来源，`priority`／`set_priority` 查询和修改有符号优先级，同级按无符号 key 排序。`Runtime::images_mut()` 访问同一存储，`update_image_list(list)` 更新并提交脏矩形，`redraw_image_list(list)` 重绘后整幅提交，均返回是否复制了画布像素；上下文关闭时仍更新自有图像并消费脏区。`new_tiled` 设置面块步长，重叠区保留重复混合，`clear` 立即清空自有图像。

`GraphicsFactory` 同时创建类型 15 的 `ImageListObject`，接入 285～287、289～299、301、304～308、310 和零裁剪 316。任务主体自动更新列表；285 自动创建隐藏子平面，291 查询输出包装，292／310 控制提交与自动更新，295 通过可选缓冲设置条目裁剪，对应 `ImageList::set_clip`。286 已接入底层像素及原始数据两种来源，自定义宿主通过 `image_data` 借用原始数据字节，并提供 `images_mut`、`create_task_child`、`canvas_layout` 和 `update_image_list`，后者先更新并消费脏区，再按提交和强制开关复制画布，返回是否复制像素。`set_source_identity` 保存不增加引用的来源标识，来源重登记保留该标识；`find_source_identity` 按有符号 key 选择首项，`find_pixels(pixels, images)` 查询当前像素身份；307／308 分别使用这两类查询，找到时向输出缓冲写槽号，缺失保持缓冲。完整参数、未实现来源及确定性初始偏移见[图像列表任务](graphics.md#图像列表任务)。

`ImageList::load_animation` 为已有条目装载 ANM，`animation` 提供只读状态；`set_animation_time`／`set_animation_playing` 设置控制，`update_animations`／`update_animations_with_random` 推进整轮并返回是否完成。续跑保留时刻和游标，期间绘制和相关属性修改返回诊断；释放可取消。299 以完整名称读取动画，缺失保留旧动画，格式错误移除旧时间线。301 设置目标时间与播放状态后推进整个列表，自动主体则在绘制前推进；宿主通过 `update_image_list_animations` 对接同一图像存储和随机源，Runtime 与 KCS 共用随机序列。`restart_animation`／`restart_animation_with_random` 强制重启单个条目，保留播放开关，返回是否完成；同 key 续跑不重复重启。306 通过宿主 `restart_image_list_animation` 接入这一服务，即使无动画或结果隐藏也标记重绘区域。两类时钟、显隐与脏区规则见[列表动画](graphics.md#列表动画与任务时钟)。

112 无需读取服务，按两个尺寸参数创建全零 BGRA8 图像，状态 1 返回句柄；画布、偏移、基点及帧 ID 为零。114 显式释放解码或空白图像，以状态 2 保留操作栈原值，零句柄跳过。释放后的句柄不复用，任务回收仅释放剩余图像。`Image::new`、`Images::create` 提供独立创建；`release_for(caller, handle)` 保留 114 的调用任务语义，`release(handle)` 解除所属任务登记并释放一个引用。跨任务释放只减少引用，原任务登记继续保留；包装到最后一个引用释放才失效。`Images::share(handle)` 为同一像素创建独立、无任务登记的包装，新句柄须显式释放；原包装释放后新包装继续有效，像素修改相互可见。

115 接受目标句柄、目标位置和请求尺寸、源句柄及源矩形。非零目标使用 `Images::blend`，叠加源偏移并按底端起始坐标混合；同图重叠保留即时反馈。零目标使用 `DrawingContext::copy_image`，以全局源矩形直接复制 BGRA，行方向与图像混合分别保留。两条路径均不缩放，状态 2 不压栈；空源只需前六项并直接跳过，非零源需要完整十项。

`drawing()`／`drawing_mut()` 查询或设置当前 `graphics::DrawingContext`，包含模式、启用／绘图开关和可选 `RenderTarget`。117 的零句柄分支接受五个参数或保留第六项，按顶端起始坐标将目标区域的颜色及 Alpha 清零，第六项忽略。上下文未开启或没有目标时跳过，状态仍为 2。渲染目标提供分块像素与更新标记，普通 8 的配置尺寸继续由 `HostState.surface_size` 独立提供；平台呈现和对象更新尚未自动驱动上下文。

普通 739 接受两个 VM 输出地址，由 Runtime 使用共享 `input_coordinates()` 和当前 drawing 的 mode／enabled 查询设备偏移经过截断、回绕加倍及逻辑转换的结果。active 与目标存在性不参与该查询；地址零有效，同址或部分重叠保持逐次读写的顺序，返回状态二且不压值。换算及异常浮点规则见[输入坐标](input.md#坐标转换)。

`graphics::Transform` 提供基点、平移、缩放、弧度旋转及显式矩阵的查询／设置和 dirty 消费。`transform_point` 返回本对象变换后的四维顶点，`transform_point_with_parent` 再应用父级矩阵；两者保留原 W，不执行 W 除法。默认基点为 `[0, 0, 5000]`，基点 Z 不参与该顶点变换。比较规则与矩阵组合契约见[绘图变换](graphics.md#绘图变换)。

`graphics::Projection` 将对象／父级变换后的三角形转换为 `ProjectedVertex`，保留独立 RHW、颜色、平面／透视分支和两级坐标变换。`Drawable::projected_triangle` 提供组合入口；调用者显式给出投影原点与开关，实际摄像及坐标组配置尚未自动装载。参数契约见[绘图投影](graphics.md#绘图投影)。

`graphics::Drawable` 保存非纹理或画布三角形、独立原始边界及绘图属性，`rectangle` 提供矩形几何，`Drawables` 按编号登记、查询和删除，`order` 提供按基点 Z 稳定归并后的顺序。`Runtime::drawables()`／`drawables_mut()` 与具体二维对象共用该登记；对象释放时清理同编号登记。属性设置、颜色覆盖及非有限优先级契约见[绘图登记](graphics.md#绘图登记与三角形)。

`Runtime::render_triangles(projection, parent)` 在已开启的当前绘图上下文中绘制基础纯色及渐变图元，返回是否覆盖目标；调用者在 update 后显式调用并负责目标清空和设备呈现。`Drawables`、`DrawingContext` 提供同名独立入口，`Surface`／`RenderTarget::blend_triangle` 提供二维纯色核心。显示门控、像素中心、Alpha 和当前限制见[纯色绘制](graphics.md#纯色三角形绘制)。 `Surface`／`RenderTarget::blend_projected_triangle` 提供 RHW 颜色插值，`blend_textured_triangle` 以同一组权重插值 UV，使用 `TextureFilter` 的镜像点取样或双线性过滤；参数与限制见[纹理与插值](graphics.md#纹理与顶点颜色插值)。

`Drawable::from_canvas(data, count)` 读取显式画布三角形，`CanvasTriangle` 保留 tag、面块索引、顶点和 UV；平面命令 83 使用显式缓冲区接入。`Drawables::render_frame` 读取画布并写入独立目标，`Runtime::render_frame` 使用运行时当前画布。提交、缺失纹理和过滤约束见[画布图元与帧](graphics.md#画布图元与独立帧提交)。

`CanvasLayout::new(tile_size, grid, step)` 与 `triangulate(source)` 将已变换的源几何切成逐块画布三角形，保留原顺序、颜色及局部 UV。步长、整数裁剪门控和资源上限见[画布几何切割](graphics.md#画布几何切割)。`Drawable::from_canvas_source` 与平面命令 41 保留源和包装两套变换；源变化后通过 `prepare_canvas` 更新，帧提交自动准备可见对象。直接查询与属性选择契约见[画布纹理包装](graphics.md#画布纹理包装)。

对象实现 `runtime::TaskObject`：基类控制与时钟、可选 VM、创建上下文、主体、对象命令、排队与同步消息、删除通知和释放。默认空操作的 `begin_update()` 在时钟和子任务更新前执行，用于按钮的每轮响应重置。除对象命令外，返回 `false` 的方法表示本次预算耗尽，续跑由运行时完成；对象命令返回 `task::CommandOutcome`，完成状态与传播状态分开表达。库提供 `BaseObject`（只有控制与时钟）与 `OrdinaryObject`（可选的 `OrdinaryTask` 及未装载状态）。`ObjectFactory::create` 按类型编号创建对象；`KcsFactory` 经 `ProgramLoader` 按名称装载普通 KCS，`external_bytes` 指定 VM 外部区容量。`graphics::GraphicsFactory { fallback }` 创建类型 2 的 `PlaneObject`，其他类型委托 fallback；基础平面提供基类控制、几何及查询、显示与输入绑定、事件响应、内部子任务传播及绘图登记清理，运动等缺失入口返回诊断，详见[二维任务对象](graphics.md#基础二维任务对象)。`input::InputFactory { fallback }` 创建类型 11 的 InputObject、类型 6 的 ButtonObject、类型 42 的 SliderObject 和类型 41 的 Scroll2Object，提供设备输入、矩形按钮及滑动控件事件；命令与约束见[输入接口](input.md)。文字类型由 `message::MessageFactory` 提供，声音类型由 `audio::SoundFactory` 提供；`kind` 保存基线类型编号。

`runtime::fes::FesFactory { fallback }` 创建未装载的类型 34 任务。337 初始化时创建空执行上下文，之后可通过 338 的文件名缓冲区装载、340 绑定可达共享列表、339 立即启动；338 与 340 可以交换先后，空场景及装载失败后的场景同样可以启动。此后由 `Runtime::update` 自动推进；首次成功装载保留已有参数、系统值、帧起点和等待期限。338 接受压缩容器或首零截止的原始文本，空文件有效，文件缺失保留 −2 属性。337 的第二数据项用零表示运行时隐式共享状态，338 和 25／26 的字节数据通过 `CommandBuffers.data1` 提供。346／347 以第一数据项选择本 FES 的局部数值槽，346 以第二项写入，347 返回当前值；支持初始化后、装载前的参数准备，编号越界时忽略写入或返回零。命令、预算、错误及当前支持范围见[图形文档的 FES 任务对象](graphics.md#fes-任务对象)。TaskObject 的 `creation_context()` 为上下文继承提供只读视图，`accepts_deferred_delete()` 为消费 TaskEffects 的对象开放嵌套删除请求；两者默认不提供。

FES 的 `prepare_release` 在子任务释放后，先向非零的共享列表发送 292 恢复提交，再向非零的辅助输入按钮发送删除命令 3。此清理不依赖初始化状态或 DRAW 是否完成，保证转场中销毁 FES 后其他场景仍可使用共享画布。接收者已释放时继续完成；路由返回错误时保留失败项，重试不重复已完成项。

普通 447 接收目标和格式字符串地址，展开后通过资源服务的 `fes` 类别补全，再依序发送 337、338；空名称仍调用补全。448 接收目标并发送 339；449 接收目标与列表编号，发送 340。三项均以状态二继续，保留 VM 返回槽，零目标仍使用完整参数路由。资源补全失败返回诊断，不复用原进程的全局临时名称内容。

630 接收 FES 目标、块名格式地址、对象前缀格式地址及原始银行值，依序展开两个名称后发送 343；空前缀不绑定对象，空块名仍路由。前缀最多 31 个 CP932 字节。调用同步完成，以状态二保持脚本栈；任务命令的显式缓冲区及块内等待规则见[图形文档](graphics.md#fes-任务对象)。

645 接受 FES 目标、参数索引和原始 DWORD，发送 346 并以状态二保持脚本栈。646 接受目标和索引，经 347 返回原始 DWORD，状态一；索引按无符号值解释，超出 512 项时写入忽略、读取返回零。参数由 FES 的反斜线引用读取。查询要求可达、未删除且已初始化的 FES 对象；原路由未写结果时会返回进程地址，库对这类未提供处理器的目标返回诊断。当前也不支持查询正在执行的 FES 任务。

普通 452 接收目标编号并发送全零数据的命令 23，以状态一返回原始任务属性。目标缺失或未写入时返回 `0xffffffff`，零目标仍进入路由；同步分派遵守运行时对执行中对象的访问约束。

Scene 已处理 `execfes`、`endfes` 及 `endkcs` 的结束时序。`execfes` 使用同一资源服务补全名称，继承创建上下文和共享列表，传递最多十六个数值参数，立即启动直属子任务后在同一时刻继续父脚本；预算和资源错误续跑保留创建进度及参数。`endfes` 在正常主体中请求子任务删除，普通主体等到下一时刻再继续；事件处理器中的结束或创建请求留待正常主体消费。`Scene::child_fes()` 查询保存的子任务编号，退出码不会自动回收子任务，后续状态通过 `_EXITCODE_` 提供；完整协议见[图形文档](graphics.md#fes-任务对象)。`execkcs` 构造普通子任务后补全 KCS 名称，依序执行 161 的装载及初始化、172 的空外部区绑定，再保存编号并继续父脚本。`Scene::child_kcs()` 查询保存编号，额外词项不求值。

`RootObject::new(input_task, sound_task)` 保存输入与声音任务编号，命令 34／35 原样返回相应值；任务删除后不会自动选择替代编号。该对象作为 Runtime 的显式根参数使用，子任务由调用者创建，自动启动链仍待接入。根对象遵循专属命令消费规则，其余未实现的专属入口返回诊断。

`runtime::fes::Scene::new(program, handles)` 组合预先装载的 FES 解释器和对象存储；与 Runtime 共用 `runtime.host().handle_allocator()`。调用前通过 `objects_mut()?.set_image_list(list, clip)` 绑定共享列表；`Runtime::update_fes_scene(owner, &mut scene, now)` 消费已支持的脚本事件并按声明顺序更新对象，返回 false 时继续调用以完成预算续跑，期间保留原时刻和游标。true 表示本轮完成，`exit_code()` 在本轮最终对象更新完成后返回最近已处理的退出通知值，不表示脚本停止。`deadline()` 查询原始等待期限，包含根运动 `0xffffffff`、绝对帧 `0xfffffffe` 和对象 `0xfffffffc`；完成恢复时点见[完成等待](graphics.md#fes-运动与帧完成等待)。`raw_input()` 查询控制器保存的原始输入掩码，`is_pending()` 查询续跑状态，`objects()`／`interpreter()` 提供只读查询；推进未完成时 `objects_mut()` 返回诊断。首次推进绑定 owner，后续不能更换。调用方在所属任务主体时点推进，保证该任务时钟已经更新；下层动画命令继续使用任务自身时钟。支持范围和未接入服务见[场景推进](graphics.md#fes-场景推进)。

`Scene::prepare(&mut state, budget)` 只消费装载期声明和对象属性，返回 false 后继续调用；完成时尚未进入 START，也未创建下层媒体任务。可以重复调用。使用与 Runtime 相同的 `numbers_mut()`，初始化表达式的副作用会立即保留。`Runtime::prepare_fes_scene(owner, &mut scene)` 在此基础上先读取 PRELOAD 的图像原始字节，再创建图像空包装及 BUTTON／SLIDER／SCROLL2 的直属类型 6／42／41 任务，不执行 START、解码图像或登记输入区域；false 后继续调用，已完成的读取与创建不重复。每条 CIMAGE／IMAGE／BUTTON 记录分配独立包装，BUTTON 另创建直属任务，SLIDER／SCROLL2 分别创建独立控件任务；后一次覆盖对象的当前包装及任务编号，记录保留各自绑定用于更新、事件反查与回收。普通 update 也会在进入 START 前完成这一准备。退出通知由任务层写入属性槽，脚本继续到等待或块末尾；后续 update 按普通规则推进。每条新通知可覆盖属性，期间的外部改写保持到下一条通知；场景本身不删除所属任务。

`Scene::keyblocks()` 查询装载时解码的第一段 KEYBLOCK；在 `prepare` 完成后使用。表的数值表达式只执行一次，后续同名声明不覆盖。`Objects::navigate_button(blocks, source, direction)` 提供无副作用的基础 BUTTON 网格选择，方向 0～3 为上／下／左／右，返回可选目标名称和银行。缺失目标、非 BUTTON、零 ENABLE 或负网格号返回 None；无效方向、越界网格号或搜索超过查询上限返回诊断。选择保留基线的名称顺序、水平查找和距离规则，KEY_* 配置不参与此版本的选择，详见[焦点网格](fes.md#焦点网格)。

`Objects::navigate_button_with_controls(blocks, source, direction)` 在当前网格没有目标时先调整 BUTTON 关联的 SLIDER 或 SCROLL2，成功返回 `NavigationOutcome::Handled`；否则继续相邻网格选择，返回 `Target(reference)` 或 `Unhandled`。`Services::navigate_button(scene, source, direction)` 默认使用这一入口。`ButtonState::set_control(Some(reference))` 保存名称／银行，None 解除关联，脚本 `attach` 使用同一状态；目标查找推迟到导航时。SCROLL2 的范围终点扣除页长，采用 i32 回绕；范围请求消费和方向规则见 [SLIDER](graphics.md#fes-slider-属性核心)。FesObject 负责起点命中、默认焦点回退、方向命名值、输入 139 移动请求和媒体刷新；成功导航跳过默认 95 响应，实际焦点事件由后续输入更新产生。命中、坐标和失败续跑规则见[方向输入](graphics.md#fes-方向输入与指针定位)。

FesObject 的通用按下键槽按媒体声明顺序扫描当前启用的 SYSKEY 匹配按钮，逐个分派 PUSH_L；重复声明保留，后续记录读取处理器修改后的属性。至少一个对象自身处理器存在时跳过默认响应，DEFAULT 单独存在仍响应。释放使用保存的按下目标，不扫描 SYSKEY；扫描游标与当前处理器支持预算和错误续跑，详见[系统键分派](graphics.md#fes-系统键分派)。

`Runtime::dispatch_fes_system_block(owner, &mut scene, name)` 同步执行 `_FES_SLEEP_`／`_FES_WAKE_`，返回 false 后以同一名称续跑；普通推进、对象修改及另一个系统块须等到本次完成。缺失块直接完成。该入口不推进系统帧，不切换媒体显示；通常保留主流程位置和等待，`next` 按系统块的调用深度规则处理。局部 `wait` 结束本次分派；系统块 `exit` 结束本次分派并将返回码排队，到下一次普通场景推进才发布。系统块也可启动已支持类型的运动；decode 结束当前处理片段并把媒体提交请求留到普通更新。尚未接入的内置命令和其他类型的运动返回诊断并保留待处理事件。命令 348 在系统块后刷新对象并切换相应类型的显示与输入启用；SLIDER 使用 367／366、SCROLL2 使用 375／379 关闭并恢复当前属性，`FesObject::paused()` 查询专属暂停原始值；完整契约见[暂停与唤醒](graphics.md#fes-专属暂停与唤醒)。

FES `dbgstr text` 与 `debug expression` 在主体、系统块及对象处理器中向 `HostState::debug_messages` 追加原始编码消息，分别使用 `dbgstr = ` 前缀和 `debug = ` 加有符号十进制结果。只读取第一个参数，缺参时跳过；参数已完成 FES 字符串展开，文本中的百分号不作为格式符再次解释。debug 沿用当前 FES 表达式上下文及副作用。

对存在的目标对象调用未注册名称时，Runtime 追加 `Unknown FES object command: ` 和展开后的整行字节后继续，后续参数不求值；只有目标名时直接继续。名称按大小写匹配，内置名称与注册绑定均不进入此回退；尚未实现的内置命令继续诊断。

Scene 已连接全局 `baseimg [object]` 的图像背景与 PLANE 截图请求，消费时向共享列表发送 302；主体先处理背景再继续脚本，系统块与对象处理器的请求在后续普通更新消费。失败与预算续跑保留来源和查询阶段。列表持有独立背景像素，清除显示后仍保存快照；详见[背景快照](graphics.md#背景快照)。

FES `strvar index text [integer ...]` 已接入共享 `expression::State::strings`，支持原始字节、最多四个整数格式参数及省略文本时清空；`strvar get index source` 按编号复制，get 忽略 ASCII 大小写。主体、系统块和对象事件共用当前表达式上下文与字符串表。格式、容量、缺参及首零规则见[运行时字符串写入](fes.md#运行时字符串写入)。

`Runtime::dispatch_fes_event(owner, &mut scene, target, event)` 分派控制器输入事件。`target` 为可选 `ObjectReference`；事件编号为 0～21，其中 0／1／2／3／4／5／8／21 分别对应 FOCUS／UNFOCUS／PUSH_L／PUSH_R／RELEASE_L／RELEASE_R／SCRL_P／TOUCH，其余编号只更新系统输入命名值。命中目标时，系统按下、方向释放和触碰值使用对象句柄；无目标为 −1，缺失目标跳过写入，左右点击释放仍为零。返回 `None` 表示预算耗尽，须以相同目标和编号续跑；`Some(bool)` 表示完成，布尔值只取对象自身处理器是否存在，默认事件组单独执行时为 false。该入口执行对象命令并可加入运动，不在分派末尾自动刷新媒体，不修改任务时钟或系统帧。局部 wait／decode 结束当前处理片段，decode 的媒体提交请求留到普通更新；DEFAULT 片段结束后仍可进入对象自身处理器。exit 另将通知排队；主等待、调用及 next 规则与解释器一致。失败时保留游标，普通更新、系统块和其他对象事件须待本次完成。

`Services::dispatch_control_event(scene, target, event, budget)` 接收滑动控件的同步通知，默认委托普通事件入口。运行时在媒体命令返回前完成事件脚本，并保留回调对当前场景的修改；`decode` 等待中的主流程位置及调用栈在处理器结束后恢复。该入口不会自动刷新媒体，也不开放事件处理器内部再次分派事件。公开 `Runtime::dispatch_fes_event` 继续要求当前推进已经完成。

`runtime::fes::Objects::new(handles)` 为一份 FES 建立独立的对象存储，并登记 `_TOPOBJ_` 银行 0 的根 BASE 对象；与 Runtime 组合时传入 `runtime.host().handle_allocator()`。FES 对象标识来自共享不透明编号源，与任务编号分别使用。`define(&definition)` 接受有效的非零银行数，返回此次新创建的全部标识；目前支持 BASE、CIMAGE、IMAGE、BUTTON、CPLANE、PLANE、SLIDER、SCROLL2、SOUND 和 DRAW 的对象状态，其他类型返回诊断。SLIDER 已接入状态、数值命令、settbl 帧表、运动、关联导航及媒体准备／更新，父级输入通知及媒体更新中的同步回调均执行事件脚本。名称限定为 1～31 个非零 ASCII 字节。`get`／`get_mut` 查询实际对象及 `ObjectState`，`len` 包括根及重复声明的未绑定对象。`MediaRecord::kind()` 保存声明类型，`task_id()` 查询 BUTTON／SLIDER／SCROLL2／DRAW 下层编号；`Object::slider_task_ids()` 查询全部声明创建的滑块任务。`update_order()` 按声明及银行顺序查询当前绑定，保留重复记录并跳过缺失银行；新声明占据银行空洞后，旧记录也会解析到新绑定。

`graphics::FadeObject` 由 `GraphicsFactory` 按类型 27 创建，接收 281 准备、263 启动、264／267 停止、265 播放状态查询及 266 来源替换。图像句柄由调用者持有，任务将过渡像素提交到 Runtime 的画布；`Host::effect_speed()` 缺省返回零，Runtime 从全局效果配置读取。参数记录、坐标、透明 RGB、时间和返回槽规则见[画布淡入淡出任务](graphics.md#画布淡入淡出任务)。FES `draw` 已连接临时 DRAW、来源截图、共享列表提交开关及完成清理；`runtime::fes::DrawState` 提供持续时间和媒体阶段查询。

`fes::SoundState` 保存 SOUND 的声明／运动属性及装载、动作请求，`subcommand_context()` 提供声音自定义运动参数的零向量、单位缩放及音量 Alpha，`load_parameters`、`play_parameters` 保留原始参数，`action_request()` 查询待消费动作。空闲 SOUND 可参与场景媒体顺序；文件请求、新声音配置下的音量／声像提交已路由到声音任务，播放动作及自身 PLAY 处理器已连接，旧配置属性与组通道自动回收继续接入。初始化及对象 load／play／channel／stype 的求值与银行规则见[声音文档](audio.md#fes-sound-对象命令)。

FES `blend` 已连接 SOUND、CIMAGE／IMAGE／BUTTON、CPLANE／PLANE 和 SLIDER／SCROLL2：先保存音量或透明度，再装载默认运动，保留数组银行、一次求值、运动基准、延迟和失败续跑。平面保存完整颜色，默认运动保留 RGB 参数。主体、系统块及对象处理器共用该入口。完整参数与更新时序见[FES blend](audio.md#fes-blend)。

`fes::Scroll2State { control, page }` 在通用滑动属性之外保存独立页长；`set_page(range, page)` 请求范围更新。`ObjectState::Scroll2` 支持初始化、数值及帧表命令和 Alpha 运动，操作示例与参数规则见 [SCROLL2](graphics.md#fes-scroll2-属性核心)。FES 侧经 `ObjectFactory` 创建类型 41、名称为 `FE scroll2` 的直属任务，并完成媒体、导航、父级通知及暂停协议；`InputFactory` 已提供这一类型的 `Scroll2Object`。`Object::scroll2_task_ids()` 查询该对象全部声明创建的下层编号。

`lookup(name, bank)` 精确查找名称／银行，缺失返回 None；−1 在这里不展开数组。重复 DEFINE 仍创建对象，但已有银行保留原绑定，新银行取得各自的首次绑定；`count(name)` 始终保留首次登记的数组数量。创建完成后先调用 `publish_bindings(&mut interpreter)`，再确认事件；该入口同步对象命名值和解释器的数组数量，保留其他系统命名值。删除后同样需要同步。显式银行可访问超出首次数量的新绑定，省略银行号的数组展开继续使用首次数量。

`initialize(&initialization, transform_name)` 按银行查找实际类型，先应用 PARENT，再复用各类型的属性规则；IMAGE／BUTTON 的非空 FILE 通过闭包转换。PARENT 的精确查找无结果时解除父级。`set_parent(handle, parent)` 可显式重挂或解除 FES 父级，循环、无效标识及根重挂返回诊断。该关系独立于任务树。`remove(handle)` 移出对象及后代，按后代先于父对象的顺序返回所有权；数量表保留、绑定移除、未绑定的重复对象不自动补位。根对象随存储释放。直接调用 remove 时，调用方负责返回对象所持的媒体资源和下层任务清理；`Object::image_handles()` 与 `button_task_ids()` 遍历其全部包装和按钮，包括重复声明的绑定，`image_handle()`／`task_id()` 只查询当前绑定。对象存储不自行推进 FES 主体。初始化错误保留先前已完成的银行及属性修改。

`fes::CPlaneState` 通过只读查询提供位置、基点、旋转、缩放、矩阵、尺寸、优先级、颜色、显示、创建类型、EXPOS、资源／VIEWPORT 引用及更新标记。`apply(&attributes, bank)` 返回本次是否使属性失效；`Objects::initialize` 同时标记 FES 后代。`motion_properties()`／`apply_motion_properties()` 捕获及回写平面运动，`subcommand_context()` 提供本地属性换算上下文；PLANE 的同名上下文另使用 VRAM。尺寸重建与属性失效独立，具体运动规则见[对象运动](graphics.md#fes-对象运动)。

`Runtime::update_fes_object(owner, &mut objects, handle)` 显式推进一个对象。BASE 无媒体操作，CPLANE／PLANE 使用实际对象工厂和命令路由创建／更新直属二维任务，`Object::task_id()` 返回可选编号。CIMAGE 与 IMAGE 通过共享类型 15 列表更新来源与登记，`Object::image_handle()` 查询可选来源包装；IMAGE 支持按当前映射帧装载普通 HG-3 文件及多文件合成，缺失文件或帧清空像素绑定。非零 PATHMODE 使用完整直接路径，跳过类别补全；IDN=−1 的原始来源与像素独立保存，具体契约见下文。调用者按 FES 对象更新顺序推进，并将 objects 与 owner 保持同一生命周期，下层任务由此入口维护。CPLANE 在创建下层任务前读取 SIZE2，PLANE 在创建后读取；POS2 在属性更新时消费；读取共用 Runtime 的资源服务，按 image 类别补全名称，支持 HG-3 元数据。已有绑定的 VIEWPORT 更新返回命令 89 的诊断。此入口尚不执行运动和 FES 主体调度。

`Objects::preloaded_image(full_name)` 以补全后的完整名称查询当前场景的图像原始字节，区分大小写并在首个零字节结束。返回 `None` 表示未缓存，`Some(&[])` 表示成功读取的空文件。预加载错误保留已完成行和当前行的补全名称，继续调用准备入口即可重试；期间 `Scene::is_pending()` 为 true，禁止通过 `objects_mut()` 修改。直接调用 `update_fes_scene` 也会在进入 START 前完成预加载。缓存范围及使用规则见[图像预加载](graphics.md#fes-图像预加载)。

`Objects::media_records()` 按声明／银行顺序提供 `MediaRecord`，通过 `target()`、`image_handle()`、`task_id()` 查询名称引用、包装及该记录创建的 BUTTON 编号；其他类型的记录 task_id 为 None。`Runtime::update_fes_record(owner, objects, index)` 用当前名称绑定和记录包装推进；Scene 普通收尾、decode 及暂停刷新自动采用这一入口。`update_fes_object` 则使用对象的当前包装。先完成 `prepare_fes_scene` 再显式逐记录更新，可保留装载早于更新的时序；记录编号越界返回诊断。类型的不同取图规则见[媒体记录](graphics.md#fes-媒体记录与来源包装)。

BUTTON 先通过 `prepare_fes_scene` 或任务 338 完成下层创建，再由对象更新复用 IMAGE 并维护实际输入区域、启用值、优先级及可选 Alpha 掩码。`Objects::set_input_task(id)` 显式绑定输入任务，`input_task()` 查询当前绑定；缺省由根命令 34 查询。输入服务错误保留尚未提交的按钮更新，后续以同一场景续跑。脚本 enable 将非零归一为一，初始化 ENABLE 保留原始值；区域与显示分别维护。对象事件通过任务 9／10／64／65 或显式场景分派接入，网格导航与坐标锁已接入，拖动命令继续诊断；详细行为见[BUTTON 区域更新](graphics.md#fes-button-区域更新)。

FES IMAGE 的 `IDN=−1` 接收完整名称的原始来源，不受 PATHMODE 影响；`Object::raw_image_handle` 返回其非持有查询句柄，零表示没有来源。像素和原始数据独立保留，登记优先使用像素；普通装载缺帧后可退回旧原始来源。FES 持有的数据随替换、对象删除或所属任务实际回收释放，列表已经解码的帧继续存活。原始来源登记以当前更新记录的包装保存来源身份，可用 308 反查；具体几何和槽号规则见[原始来源更新](graphics.md#fes-image-原始来源更新)。

`Objects::dispatch_object_command(interpreter, line, state)` 消费 `disp`／`size`／`keypos`／`setid`／`getid`／`enable`／`pri`／`pos2`／`delay`／`ismot`／`attach`／`settbl`／`pl`／`lock`、图像／平面的 `getpos`，以及滑动控件的 `pos`／`page`／`getpos`／`load`。返回 true 后调用者确认事件，其他命令返回 false。目标银行与数值参数使用 FES 自身作用域，图形请求在对象更新时提交；getid／getpos 即时写入共享变量。起始目标缺失时，对行级展开后的原始整行数值求值，保留 FES 作用域与连续操作数规则。Scene 另接图像／平面的 pos 和默认 p 运动，在主体、系统块及对象事件中提供预算续跑与资源重试；独立 Objects 对该路径返回场景服务诊断。位置查询使用本地坐标并按 x86 规则取整，缺省轴、数组、类型和资源时点见[对象控制命令](graphics.md#fes-对象控制命令)。

Scene 消费 `keyskip` 并即时更新等待取消模式，`forced_key_skip()` 查询保存的 force 标志；它与当前输入模式分别维护，完整规则见[等待取消](graphics.md#fes-等待取消与-keyskip)。

Scene 消费所有已接入对象类型的 `arc`／`mcarc`，装载默认 m 运动并分别加入分组零／一。参数、运动延迟、基值捕获和已完成银行在资源失败后保留；缺失资源继续脚本。命令适用于普通主体、系统块与对象事件，具体参数及银行规则见[对象控制命令](graphics.md#fes-对象控制命令)。

Scene 消费 `noact`／`lockactive` 并按主体时点处理通知。`preferred_focus()` 返回已生效的默认焦点引用，`focus_lock()` 返回已生效的原始整数锁值，均不包含尚在排队的修改。完整规则见[默认焦点与焦点锁](graphics.md#fes-默认焦点与焦点锁)。

`Objects::dispatch_image_animation(interpreter, line, state)` 消费 `Event::Command` 中的 IMAGE `setanm`／`anm` 请求；返回 true 后可确认事件，其他命令返回 false。`Object::image_animation_request()` 查询保存的名称、播放开关及待处理标记。命令阶段不装载，`update_fes_object` 在来源登记后、排序前消费请求；播放重新读取动画并启动，暂停只更新开关。完整参数、类型门控及失败契约见 [FES 动画请求](graphics.md#fes-image-动画请求)。

`fes::PlaneState` 的 `base` 复用平面属性，`vram()` 返回独立的画布取样位置，`apply(&attributes, bank)` 另应用 VRAM。运行时 PLANE 需要先在 DrawingContext 提供画布目标，再通过 `render_frame` 输出；支持跨面块取样。`Objects::set_canvas_offset([x, y])` 保存共享绘制服务给出的画布偏移，`canvas_offset()` 查询该值；设置本身不使对象失效，在下一次 PLANE 属性更新时叠加到 VRAM。PLANE 的尺寸重建保留有效屏幕位置，其他属性仍等到失效后提交。当前支持 CPLANE／PLANE 父级的位置累加，基点仅沿直接为 PLANE 的父级累加。

`Objects::set_image_list(task, clip)` 为 CIMAGE／IMAGE 绑定已初始化的共享列表和全局裁剪；列表必须是 owner 通过树路由可达的类型 15 任务。更新按来源重建、属性登记及排序分别消费标记，支持 POS2／SIZE2、CIMAGE／IMAGE 父级位置及 PLANE 裁剪；具体时序见 [CIMAGE](graphics.md#fes-cimage-更新) 与 [IMAGE](graphics.md#fes-image-文件更新) 更新。`Objects::invalidate(handle)` 在直接显示属性修改或运动回写后标记对象及后代；直接来源修改还需请求 rebuild。

`Objects::start_motion(handle, program, parameters, delay)` 向对象加入分组零的运动，返回对象内的运动编号；当前支持 BASE、DRAW、CIMAGE、IMAGE、BUTTON、CPLANE、PLANE、SLIDER、SCROLL2 和 SOUND。目标及其后代都没有活动运动时，从对象当前属性捕获基准；有活动运动时沿用已有基准。`motion_active(handle)` 查询目标及其后代，`Object::motions()` 只读查询目标自身队列，`motion_delay()` 查询脚本为后续运动保存的原始开始偏移。`delay` 不改变已经插入的运动；`ismot` 包含尚未到开始时刻的活动条目。`update_motions(now, &mut host, budget_per_task)` 显式按先子后父的顺序推进根对象子树，并回写对象属性及后代失效标记；false 后继续调用，时间、已完成对象与当前队列均保留。续跑期间不得直接修改属性，创建、初始化、重挂、删除及加入运动会返回诊断。Scene 在等待或帧边界自动调用该入口，再按声明顺序刷新媒体；普通 decode 的中间提交先刷新媒体并继续脚本，运动每轮最终阶段执行一次。处理器只加入运动，后续普通更新才执行。失败重试保留已完成的运动阶段；运动与普通 KCS 共用随机序列和调试消息，详见[对象运动](graphics.md#fes-对象运动)。

`Objects::request_motion_finish(handle)` 仅向目标及后代请求结束，保留队列和延迟，供后续普通更新执行；已有运动续跑时返回诊断。

`Objects::stop_motions(handle, &mut host, budget_per_task)` 请求目标及后代结束，以零时刻更新子树，再清空目标自身队列和运动延迟。false 后继续同一目标的停止；此时普通运动更新或其他目标停止返回诊断。后代队列和延迟保留，运动脚本自行响应结束请求。Scene 的 `object stop` 逐银行调用，并支持事件对象 `@` 和银行空洞；媒体仍在后续提交边界刷新。

`Runtime::remove_fes_object(owner, &mut objects, handle)` 先向下层任务发送删除标记、移除图像当前槽号并释放来源包装，再移出 FES 子树；二维绘图登记立即清理，实际任务释放发生在树更新中。删除后调用者再次 publish_bindings。更新和移除错误保留已经完成的修改；工厂创建失败清除本次重建标记并保存 None，后续尺寸请求可重新触发创建。

图形事件的同步响应通过 `Host::dispatch_graphics_event(response, command, buffers, output)` 开放。运行时回调中的命令 95 改写当前平面的响应，返回错误时也保留此前修改；其余需要重入完整平面对象的命令返回诊断。移动通知使用同一份 CommandBuffers，事件接收者返回的普通 output 与平面响应分别保存。

矩形按钮使用 `Host::dispatch_button_event(button, command, buffers, output)`。运行时在回调期间开放同一份 ButtonObject，允许同步响应、区域、启用、掩码及基类命令；错误返回后保留已完成修改，回调结束后恢复原对象及宿主身份。回调中的主体重入继续诊断；独立宿主的默认实现复用图形响应入口。

`Runtime::input_state(id)`／`input_state_mut(id)` 查询输入状态。`dispatch_input_key`、`scan_input_keys`、`update_input_selection`、`dispatch_input_motion` 将逻辑坐标和设备查询连接到真实任务命令路由。接收者回调可以修改正在执行的输入任务的区域、键开关、指针及基类状态，后续遍历立即生效，错误返回也保留已发生的修改。完整协议见[输入对象](input.md#输入对象与运行时)。

`input::HitMask::from_image(&image)` 按画布尺寸、裁剪偏移及非零 Alpha 生成区域持有的独立位图。输入命令 147 与平面命令 98 通过 `Host::image(handle)` 使用普通 111／112 的解码图像；位图在图像释放后继续有效。独立宿主提供解码图像借用，TaskHost／ResourceHost／SystemHost 向内层委托该查询。边界、替换失败和空句柄规则见[位掩码](input.md#位掩码)。

`Host::parent_task_id()` 返回当前对象的直接父级编号，默认返回诊断。Runtime 从任务树查询；CoreHost 使用其 task_tree，TaskHost 优先使用 TaskServices 的树，ResourceHost／SystemHost／`&mut H` 继续向内层委托。平面 53／54 开启／解除相对父级坐标绑定，具体规则见[图形接口](graphics.md)。父级传播期间的 61／62 可查询正在执行的平面，保持删除门控、几何源选择及输出缓冲区契约；完整平面重入仍未开放。

`Host::surface_size()` 提供普通 8 使用的渲染上下文尺寸，默认两个零；CoreHost 与 Runtime 返回 `HostState.surface_size`，三个宿主包装及 `&mut H` 向内层委托。平面关联提示的定位使用这一宽度。平面 77 保存提示任务编号并隐藏新目标，78 控制状态一的自动定位与显示；提示任务需由调用者创建，完整位置与状态切换规则见[图形接口](graphics.md)。

`DeviceKeys::from_xml(bytes)` 读取八十个逻辑键槽及四类设备的四组绑定，`new` 接受显式 `DeviceBinding` 数组。平台通过 `button` 提交按下／释放，`sample` 查询本轮状态，`commit` 提交持有态；一轮内短按保留到提交。四组是第一至第四个绑定，设备来源由掩码表示。`force` 按查询次数强制按下，额外查询同样消费计数；`clear_pressed` 保留重复元数据及强制计数。配置字段、Mouse／Touch 共用绑定和高槽门控见[物理按钮映射](input.md#物理按钮映射)。

`Runtime::set_input_device(device)` 绑定同步 `input::InputDevice`，为已有输入对象逐个查询初始位置；之后新建输入对象在登记前查询，失败不占用编号。共享映射通过 `input_coordinates()`／`input_coordinates_mut()` 配置。InputObject 主体自动执行轮询、定位、选择、按键、滚轮、拖动、光标及更新收尾；未绑定设备返回诊断，显式事件入口仍可使用。平台采集继续实现，设备协议及查询次序见[同步设备更新](input.md#同步设备更新)。独立输入宿主提供 `Host::input_device`／`input_coordinates` 和 `dispatch_input_command(state, delivery, command, buffers, output)`，delivery 区分直接分派与广播，返回值表示目标可达性。

`Runtime::set_input_pointer_control(enabled)` 设置命令 138 的配置门控，默认 false；139 仍可在前台请求有效目标。独立宿主实现 `Host::input_pointer_control`。两个命令接受打包的无符号十六位逻辑坐标，清理受抑制的键状态后保存定位目标、剩余值和任务 elapsed；设备通过 set_pointer 和 clear_keys 完成对应操作。时刻及坐标约定见[平滑定位](input.md#平滑定位)。

`InputState::set_update_rate(rate)` 设置自动隐藏命令 143 所需的任务速率，默认零；配置宿主提供实际值，`update_rate()` 查询原值。143 设置开关与阈值，159 重置空闲计数，二者按门控调用设备并广播 22。输入命令的回调修改及错误前已发生的副作用保留，详见[对象命令](input.md#对象命令)。

普通对象执行期间仍接受 67 删除请求及 70 排队消息；69 同步发送给执行中的普通 VM 时返回零。删除请求保留后续指令，实际析构继续发生在树更新；消息处理阶段追加的消息在同一轮继续消费，无消息恢复阶段追加的消息留到下一轮。`TaskObject::queued_messages()` 提供可选队列视图，支持执行中任务操作的自定义对象应提供该视图并消费 `Host::take_task_effects`；`OrdinaryObject` 已完成这些操作。给已有删除标记的对象排队时，原 native 的返回槽未初始化，运行时返回诊断且不入队。

执行中的对象暂时从对象存储取出；输入同步回调通过 `Host::dispatch_input_command` 开放同一 CommandState 处理嵌套命令，其他需要直接借用执行中对象的命令或 VM 访问继续返回诊断。嵌套执行前保存宿主的任务编号、时刻和起点，返回后恢复，外层脚本继续读取自己的任务状态。对象命令经 `TaskTree::dispatch` 路由，目标改写、可达性判断和 `data[2]` 传播沿用树契约；`dispatch_command(origin, command)` 的 `origin` 只用于路由，`command.sender` 保持调用者给出的值，natives 108／109／110 以当前任务为发送者。未识别的基类编号不修改状态并允许传播，基类命令和已有删除标记的对象消费当前分支；普通任务对已知但尚未实现的专属编号继续返回诊断。

`Runtime::dispatch_command_with_buffers(origin, command, &mut buffers)` 按相同树规则分派显式字节缓冲区。`task::CommandBuffers` 的 data0／data1 是可选可变借用，对应原 data[0]／data[1] 所引用的数据；不解释 DWORD 为进程指针或 VM 地址。各接收者共享缓冲区中的修改。对象通过 `TaskObject::dispatch_command_with_buffers` 接收，缺省委托标量入口，所需长度由具体命令检查。`Host::next_task_child(previous)` 查询当前对象在有符号编号游标之后的直属子级，None 从首项开始，`dispatch_task_command(command, buffers, output)` 保留传入返回槽并返回是否找到目标，用于对象内部传播；宿主未提供服务时返回诊断。

普通 78／79／80／82／83／84／100／101 已在运行时连接基础图元及属性命令，均以状态 2 保留栈原值；零目标只需一个 DWORD，非零缺失目标继续执行。完整参数及对象命令见[基础图元](graphics.md#普通脚本的基础图元)及 [KX2 几何](graphics.md#kx2-几何资源)。对象通过 `Host::resource_services()` 访问与 native 共用的同步资源服务，通过 `Host::canvas_layout()` 查询画布布局；未提供所需服务时返回诊断。

主体、命令和消息在返回前完成预算续跑。`DEFAULT_BUDGET` 为单次推进预算，零预算按一处理；连续耗尽预算时，`DEFAULT_LIMIT` 按续跑累计额度触发同步执行诊断。与库的其他执行错误一样，出现该错误后应丢弃解释器状态。不结束的普通指令循环因此会返回诊断。

当前运行时接入普通表任务 native 66／67／69／70／72／73／75／76／108／109／110／452 和已核实的普通表接口；108／110 接受目标编号和值两个 DWORD。`KcsFactory.external_bytes` 超过库的 64 MiB 上限时在分配前返回诊断。629 仍由 `SystemHost` 单独包装，尚未并入；367 尚未实现。`Runtime::create` 配合 `KcsFactory` 保留按名称预先装载的便利入口；`Runtime::create_unloaded_ordinary(parent, context)` 构造无 VM 的普通对象。工厂包装需转发 `create_unloaded_ordinary` 和 `load_kcs_program`，宿主包装需转发程序装载及共享数值绑定状态。命令 161 从 `CommandBuffers.data1` 读取完整名称；首参非零时立即初始化，零时只装载。缺失程序会移除旧 VM，保留任务编号、控制、时钟和队列，随后仍继承父级数值区及上下文；程序读取服务错误保留装载阶段，沿用同一命令重试。172 绑定外部区，已接入零长度绑定，非零长度仍返回诊断。

## 约束与验证

单个容器、解压主体和单块运行存储上限为 64 MiB；主要记录表上限为一百万项。表达式最多 64 KiB、512 个节点、128 层递归；参数、菜单扫描和语言表另设 4096 项上限。超限返回诊断。可续跑的宿主服务错误遵循各入口的保留契约；其他执行错误修正输入或实现后重新创建执行器。

`Error::offset` 在容器和表达式读取中为字节位置；CST 执行错误使用记录索引，ANM 使用指令索引，FES 使用文本字节位置，KCS 使用代码字节 PC。语法子解析器产生的位置相对于当前表达式或参数。

常规测试仅使用合成字节和脚本，覆盖已核实的数值副作用、条件菜单、场景返回、FES 帧重启、ANM 循环、KCS 调用／重试及非法容器。另已逐归档验证 1,812 份实际脚本实例（按名称去重后 1,286 份）的解密及容器读取；这项检查不纳入默认测试，也不分发输入材料。

`kcs::ParticleHost` 提供 KCS 2.0 粒子表中的数值、时间、网格、参数快照、源对象查询及退出接口，支持范围和参数见 [KCS 文档](kcs.md#粒子脚本宿主)。调用者在推进前设置时间、当前网格、表面尺寸及投影的 `depth_offset`／`camera_depth`，提供四边形、快照、偏移和标记数组。`ParticleVertex` 保留 XYZ、RHW、颜色和 UV。`ScriptRandom` 保存独立的整数和浮点序列，可用显式种子创建。`ParticleSource` 分别保存源对象的颜色、二维坐标和三维坐标；将查询结果写入 `source`，查询无结果时设为 `None`。`object_id` 为 −1 时使用全零源属性。FES 子命令使用另一张运动表。

`kcs::ParticleTask::new(program, host)` 创建粒子脚本实例，`update(now, budget_per_quad)` 按网格顺序从入口执行，保留共享数据区。未活动时返回 `None`，整轮完成返回 `Some(Step::Halted)`，预算不足返回 `Some(Step::Yield)` 并继续当前网格。预算按单个网格计算；续跑沿用本轮时间，期间应保留宿主时间、网格编号及数组。`origin`、`previous_time` 和整数 `speed` 控制累计时间；网格数组需预先准备且数量相同，可用 `host.reset_quads(count, image_size)` 创建矩形顶点、快照、零偏移和启用标记；该方法保留时间、活动标记和参数。源属性、表面和资源上下文由调用者提供，图形提交与父对象更新由外层负责。

`kcs::MotionHost` 提供运动表的 0～44 项接口，参数契约见 [运动脚本宿主](kcs.md#运动脚本宿主)。用 `MotionState::with_parameters` 创建参数区，以 `external_data` 创建 VM 外部区，执行后用 `MotionState::read` 读取属性及状态。`properties` 保留 36 个原始 DWORD，调用者提供对象属性布局；全部内存访问检查边界。native 38 清除 running 并让出。`MotionTask::new(program, state, base_time)` 创建运动实例；`state()` 读取当前属性及控制数据，`set_properties` 仅替换 36 项属性，保留时间、运行标志、参数和程序数据。

`MotionTask::update(now, host, budget)` 在未到开始时间且 finishing 为零时返回 `None`；否则设置按 u32 回绕的 elapsed，并从入口重新执行，返回 `Some(Step)`。`Halted` 结束本次入口执行，running 的含义由对象调度处理。预算耗尽返回 `Yield`，下一次 `update` 继续同一次执行并保留原 elapsed。宿主事件结束本次更新，下一次更新重新从入口开始。

`finish(host, budget)` 以 elapsed=`u32::MAX`、finishing=1 从入口执行，保留当前 running。完成或错误时恢复这三个字段，保留数据区、属性、参数和宿主副作用。预算让出后继续调用 `finish`；这期间 `update` 返回诊断。`finish` 可以中断尚未完成的普通更新。`Interpreter::reset_execution` 单独提供入口和栈重置，保留数据区、外部区、switch 值及调试元数据。


## 运动队列

`kcs::MotionQueue` 保存按创建顺序执行的运动实例。每次新遍历前，调用者设置 `base_properties`；`speed` 默认为 1。`insert(task, group)` 返回从 1 开始的递增编号，删除及清空后不重置编号。`task(id)` 提供只读任务状态，`group(id)`、`time_origin(id)` 查询分组及时间起点。FES 的两类 SUBCOMMAND 声明在当前基线启动路径均传入分组 0。

`update(now, skip_unstarted, host, budget_per_task)` 从基础属性开始，逐项组合结果。`update_object(now, host, budget_per_task)` 接入对象层的活动查询和统一清理；完成后在 `was_updated()` 为真时回写 `properties()`。任务尚未到自身延迟时保留传入属性。FES 对象通过 `Objects::start_motion` 在子树无活动运动时捕获基准，随后各帧复用该值。

两个更新方法和 `finish(host, budget_per_task)` 均以 true 表示本次遍历完成，false 表示预算让出。预算分别交给每个任务；续跑时调用相同方法，队列保留时间、速度、基础属性快照及已完成的组合结果。`is_pending()` 查询是否需要续跑。`finish` 可中断普通更新，从基础属性开始依次执行提前结束调用，保留队列；结束遍历期间调用更新方法返回诊断。

`any_running(None)` 查询所有任务，`Some(group)` 只查询对应分组。`request_finish(id)` 和 `request_finish_group(group)` 设置结束标志，后者的 None 作用于全部任务；这些请求在下一次更新执行。`set_time_origin(id, origin)` 可指定单个编号或用 None 修改全部起点。`remove(id)` 返回是否找到任务，`remove_group(group)` 返回移除数量，均直接删除。`clear()` 取消预算续跑并丢弃全部任务，保留编号计数和最近结果。预算续跑期间，插入、按编号／分组删除、修改起点和结束请求返回诊断。

`resource::Category { prefix, extension }` 接收配置的虚拟名前缀和缺省扩展名，`resolve(name)` 按基线 CP932 名称规则返回资源字节名。调用者负责类别选择与实际资源查找；完整契约见[资源名称文档](resources.md)。

## 资源配置

`resource::XmlDocument::parse(bytes)` 读取基线 UTF-8 XML，`root_name()` 提供根节点的字面名称。`text(path)` 返回可选借用字节，`integer(path, default)` 在缺失时采用默认值；路径和输入约束见[资源文档](resources.md#脚本-xml-字段)。读取器不访问文件系统。

`resource::ArchiveConfig::from_xml(&str)` 读取启动 XML 的类别及必需归档列表。`entries` 固定包含 128 个槽位，`required` 包含 64 个原始整数编号。`find` 按标识返回第一个匹配槽位；`resolve` 接受类别标识和资源名字节，返回虚拟名称，查找失败返回诊断。`Category::resolve` 也可独立用于显式前缀与扩展名。接口不读取文件系统。具体输入约束见[资源名称与类别](resources.md)，XML 诊断的 offset 使用零。

`resource::ArchiveDirectory::new` 索引已经解密的 CP932 目录条目；`from_plain_kif` 读取完整未加密 KIF，遇到加密标记返回诊断。`entries()` 保留输入顺序，`find`／`find_utf16` 返回首个匹配条目及范围。目录查询保留基线宽字符大小写转换步长，不规范化路径分隔符。`KifArchive` 提供数据块读取与解密。

`resource::verification_seed(code)` 从已解密校验码计算名称种子。`KifArchive::parse(bytes, seed)` 借用完整归档，解密目录并检查所有资源范围；`directory()` 提供目录，`read(name)` 返回可选明文，`read_entry(entry)` 读取指定范围。条目数据上限为 64 MiB。未加密归档忽略名称种子，末尾不足八字节的数据保持原值。校验码提取和文件系统读取由宿主完成。

`resource::ResourceRouter` 接收 256 个槽位、显式列表组及补丁顺序，挂载的 `KifArchive` 和输入数据需在读取期间存活。`read(cp932_name, mode, file)`／`read_utf16(name, mode, file)` 返回可选 `ResourceData { source, bytes }`，保留来源归档身份。`ResourceMode` 提供直接文件、归档、散装优先和归档优先四种模式，文件闭包以 `Ok(None)` 请求继续回退，错误则传播。路由前缀当前限定 ASCII，生成路径最多 259 个 UTF-16 单元；详细顺序、空前缀特例和列表契约见[资源文档](resources.md)。自动挂载、列表解析、补丁发现、缓存和别名由宿主准备。
