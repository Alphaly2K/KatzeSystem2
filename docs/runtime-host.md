# 运行库与宿主边界

KatzeSystem2 提供 CatSystem2 2.6.1.90 的 Rust 运行库。宿主创建实例，提供输入、时间和服务，推进运行并消费输出。参考 art3m1s-core 的会话、资源提供者和绘制列表边界，沿用本项目已有脚本行为、资产格式和运行状态。

|边界|运行库|宿主|
|:--|:--|:--|
|资源与持久化|解析归档和脚本、解释资源名称、编解码存档，通过读取源及逻辑操作访问数据。|选择目录或其他存储，实现资源、配置、存档、导出和脚本文件服务；处理文件路径、枚举、权限及修改时间。|
|画面|合成场景、生成有序绘制命令、记录纹理来源和尺寸；保留脚本内部像素操作与截图语义。|执行绘制和窗口呈现，持有 GPU 资源；按脚本所需提供离屏回读。现有软件绘制可作为示例和测试后端。|
|声音与电影|解码、混音、控制循环与音量，提供 PCM 及电影帧；保留任务查询和完成时序。|选择音频设备、流格式和缓冲，拉取 PCM 并呈现电影；可选提供语音合成。|
|网络与系统|按脚本协议分发请求并管理任务状态。类型 62 保留脚本包装和任务路由，默认接收者列入兼容缺口。|执行网络或系统动作，实施策略，回交结果；处理窗口、剪贴板、输入法、插件及对话框。|
|输入、字体和时钟|处理游戏输入和逻辑时钟，按字体数据完成字形度量与栅格化。|提供输入事件、运行时间和当地日期时间，发现字体并提供数据。|

嵌入时从 `katze_system2::session` 导入 `GameSetup`、`GameRuntime`、`FrameTime`、`FrameOutcome`、`StartupError` 和 `AdvanceError`。服务与对应的数据类型按下表导入，旧的 `host::类型名` 平铺路径继续兼容。

|模块|宿主接口|
|:--|:--|
|`host::resources`|`ResourceServices`、`ResourceMode`、`ArchiveConfig`、`ArchiveEntry`、`Category` 及资源缓存|
|`host::storage`|`ReadSource`、`SharedSource`、`FolderPaths`、`ScriptSettings` 及脚本文件、存档、配置和导出服务|
|`host::graphics`|`DrawList`、`DrawCommand`、`FrameTexture`、`FrameCanvas`、`GraphicsBackend`、`SequenceCapture`、`Surface`|
|`host::audio`、`host::movie`、`host::media`|`Mixer`、`Pcm`、语音合成、电影解码及媒体执行方式|
|`host::model`|模型选择、播放实例和带锚点偏移的透明图像帧|
|`host::input`、`host::font`|输入事件、按键绑定、`DeviceKeys` 和字体服务|
|`host::platform`、`host::effect`|`WindowPreferences`、窗口与剪贴板请求、平台任务及外部效果播放器|
|`host::diagnostics`|脚本位置、调试消息、native 跟踪及 `QuizDebugSession`、`QuizDebugInput`|

未识别的 3D 粒子插件可由 `host::effect::EffectServices::open_particle` 接入。服务取得资源名和字节，为当前任务创建 `ParticlePlayer`。播放器通过借用的 `ParticleFrame` 更新运行库持有的网格，可在启动或更新时清除 `active` 来结束脚本等待。活动实例收到脚本停止命令时调用 `ParticlePlayer::stop`，之后仍可重启。运行库继续生成绘制命令，由图形宿主呈现。服务不可用时，装载返回插件诊断。

示例宿主可通过 `KATZE_PARTICLE_HOST` 指定独立的 32 位插件进程。进程接收一个插件文件路径，每个实例有独立的输入输出流。仓库中的 `examples/particle-host/main.c` 可用 MinGW 编译成控制台程序。

```sh
i686-w64-mingw32-gcc -O2 -municode examples/particle-host/main.c -o particle-host.exe
KATZE_PARTICLE_HOST="$PWD/particle-host.exe" cargo run --release --example runprobe -- "$GAME_DIR"
```

在其他平台可指定能启动该控制台程序并转换路径的启动器。此示例支持文件直读的 2.2／类型一插件，并提供已核对的插值回调。其余插件回调会返回诊断。宿主进程执行游戏提供的原生代码，按宿主选择使用指定的文件。默认 Rust 库构建保持原有依赖。

宿主可从 `host::graphics::software` 导入参考软件后端 `SoftwareBackend` 和它的输出目标 `RenderTarget`。`runprobe` 用它执行离屏渲染。MPEG 解码器从 `movie` 导入。绘制命令和 `Surface` 原有的 `graphics` 导入路径仍然可用。任务树、原始命令编号、VM 缓冲区和分派表在底层 `runtime` 接口中供诊断和工具使用。Rust 契约稳定且有非 Rust 宿主接入时，再定义独立版本的 C ABI 和显式释放约定。

标准宿主用 `GameSetup::standard()` 创建根任务和清零后的脚本存储。需要指定容量时可用 `GameSetup::new(shared_slots, external_bytes)`，前者按 32 位值计数，后者按字节计数。启动前可将启动文档、游戏默认值及用户设置的 XML 字节交给 `configure_startup`，由它应用引擎与平台配置并返回游戏逻辑尺寸；配置无效时保留原有状态。绑定输入、资源和需要的其他服务，调用 `configure_canvas` 设置非零的逻辑输出尺寸并按游戏配置创建画布，再把启动配置中的脚本名称交给 `start_game(entry)`。该方法消费 `GameSetup`，并在创建任务前检查资源、输入和画布。成功后返回 `GameRuntime`，宿主可用它推进帧、呈现和拉取 PCM。宿主可先调用 `validate_startup` 检查这些条件；该检查不会访问宿主服务或装载脚本。两个方法使用 `session::StartupError` 分别报告缺少的服务；任务创建或入口装载失败由 `StartTasks` 携带原始诊断。

宿主可在启动前调用 `set_script_random_seed(u32)` 提供脚本随机种子。整数和浮点序列分别按该种子初始化。离屏示例可通过 `KATZE_SCRIPT_RANDOM_SEED` 传入同一个值，用于复现依赖随机抽取的分支。

逻辑尺寸取启动文档的 `SCREEN/width` 和 `SCREEN/height`。宿主通过 `window_preferences` 读取用户窗口尺寸、启动文档的全屏候选尺寸及当前全屏偏好，再根据设备能力决定实际模式。`configure_canvas` 同时设置呈现和输入使用的逻辑尺寸。桌面宿主按实际窗口尺寸换算鼠标坐标；离屏宿主的显式回放点击使用逻辑坐标，截图保持逻辑尺寸。

普通 KCS 的入口和后续装载使用已绑定的 `ResourceServices`。运行库按 `kcs` 类别补全名称，以 `ArchiveFirst` 模式读取字节并解析；补丁与归档顺序由资源服务决定。工具可用 `GameRuntime::into_task_runtime` 接续已启动的实例，或用 `GameSetup::into_task_runtime` 接管启动步骤。也可直接使用 `runtime::TaskRuntime::with_programs` 提供预解析程序，或用 `TaskRuntime::with_resource_programs` 绑定现有外部存储。`TaskRuntime::start_game_with_tasks(entry)` 可另外取得用于诊断的任务编号。

宿主通过 `host::resources::ResourceServices` 提供资源，通过 `host::storage` 中的 `FileServices`、`SaveServices` 和 `ConfigurationServices` 分别接入脚本文件、存档和配置持久化。资源服务返回完整字节或可独立读取的 `SharedSource`；宿主可用 `CachedResources` 为自定义装载流程复用预读缓存。

标准会话在启动前必须通过 `set_input_device` 绑定 `host::input::InputDevice`。离屏宿主可提供只返回空闲状态的内存设备。`startup_input_bindings` 返回已经应用用户覆盖的八十个逻辑键槽，宿主可以用它构造 `DeviceKeys` 或自行维护输入状态。配置保存用 `InputDevice::key_configuration` 借用 `&[DeviceBinding; 80]`；使用 `DeviceKeys` 的宿主可返回 `Some(keys.bindings())`。返回 `None` 时使用启动配置。

`GameSetup::set_game_resources` 接受资源服务与声音配置文档。运行库按声音定义选定归档类别，应用用户覆盖以控制播放；宿主按该类别补全名称并提供读取源。没有新版声音定义时，使用启动文档里的旧版文件映射和声音组。自定义声音路由的宿主可以用 `set_resources`，由其资源服务直接处理 `sound` 类别。

调用 `advance(FrameTime { tick, increment, system_time_ms })` 推进一轮。`tick` 为游戏逻辑时钟（通常每秒 60 个单位），`increment` 是本轮传给任务的逻辑增量，`system_time_ms` 为可回绕的系统毫秒计数。先处理上一轮语言请求，再推进媒体和任务。语言刷新失败时保留失败项及后续请求，已成功应用的界面刷新在下一次成功推进时报告。

`GameRuntime::advance` 使用 `session::AdvanceError` 区分语言、电影、任务和配置通知错误，`diagnostic()` 返回原始诊断。`LanguageRefresh` 可在资源恢复后重新尝试，失败的调用尚未推进任务。`ConfigurationNotification` 会在下一次完成语言和电影阶段后继续通知已推进轮次的剩余对象。`TaskUpdate` 可能已经改动状态，宿主应按诊断处理。任务工具的 `Runtime::advance` 和 `update` 仍返回原始 `Error`。

`FrameOutcome::Running { interface_language_refreshed }` 表示本轮成功。标志为真时，窗口宿主可以通过 `string_tables` 更新界面译文，再消费绘制和声音输出。`FrameOutcome::Exited` 表示根任务已退出，应结束会话循环；再次调用 `advance` 仍返回 `Exited`。需要自定义结构的调用方仍可用 `Runtime::new` 和 `update`。

窗口宿主在更新后调用 `process_window_requests(window)` 执行脚本提交的窗口请求。设备操作由 `WindowServices` 提供，运行库负责配置回写和任务通知。返回 true 表示应退出窗口循环；发生错误时可在后续调用继续处理尚未尝试的请求。文本对话框通过 `text_entry_request` 取得初始文字及请求句柄，使用同一个句柄调用 `complete_text_entry` 确认或取消。以 `Arc::ptr_eq` 区分请求；同样的初始文字可以属于不同请求。过期请求返回 false。

宿主通过 `set_archive_config`、`set_folder_paths` 和 `set_script_settings` 提供脚本可查的路径及初始配置。图片导入宿主可用 `image_import_directory` 读取、用 `set_image_import_directory` 更新最后选择的 UTF-16 路径，供随后保存配置。标准会话的窗口和目录接口取代直接借用或替换原始设置；任务工具仍可从 `settings` 访问原始设置类型。启动配置和资源服务就绪后调用 `load_startup_language_tables` 装入初始译文；没有可用的界面语言前缀时，仅装入游戏表。

窗口宿主可通过 `set_window_content_rect` 和 `set_windowed_mode` 提供客户区绘图矩形和当前窗口模式。`take_clipboard_requests` 取走按序提交的剪贴板写入请求，保留发起任务和原始编码，由宿主解码并写入剪贴板。

宿主用 `GameRuntime::present_frame` 借用当前绘制列表和 `FrameCanvas`，无需创建软件渲染目标。通过 `texture` 解析命令中的纹理，通过 `tiles` 和 `dirty_tiles` 更新画布缓存。呈现和脚本回读共用一个纹理缓存时，由宿主上传所有变更面块后调用 `acknowledge_upload`；有独立缓存的多个消费者应各自跟踪更新。

`set_graphics_backend` 设置脚本画面捕获与实时滤镜共用的离屏后端。实时模糊、马赛克和阴影在合成时同步回读中间结果，再把它们作为纹理交给最终绘制列表。`GraphicsBackend::render` 在透明背景上执行命令，返回与请求尺寸一致、从顶行开始紧密排列的 BGRA8 `Surface`。没有回读需求的会话可以不注册后端；脚本实际请求画面捕获或合成启用的实时滤镜时，缺少后端会报错。软件宿主可调用 `setup.set_graphics_backend(SoftwareBackend)`，并用 `SoftwareBackend::render_into` 复用已有呈现目标。

全屏转场的活动状态随 `advance` 更新。宿主可跳过或重复取帧，脚本无需等待呈现便能收到转场完成状态。

需要连续截图时，回调同时收到一个 `SequenceCapture`。呈现提交后调用 `SequenceCapture::write_bgra` 交回从顶行开始的 BGRA 像素及以字节计的行距。软件宿主可用 `SoftwareBackend::submit_capture` 直接提交软件渲染目标。每次请求只能提交一次；跳过呈现时可丢弃请求。运行库负责裁剪、编码 BMP 和编号，通过宿主文件服务写入。只需查看或离屏绘制时可用 `with_frame`。

离屏宿主在更新后用 `mix_audio_stereo(rate_hz, buffer)` 拉取 PCM；此调用可等待解码。设备宿主用 `connect_audio_output()` 取得可移交给音频线程的 `Mixer`，由回调用 `Mixer::render(rate_hz, buffer)` 拉取 PCM，无须借用运行实例。两者均覆写调用方缓冲，格式为左右交错、幅度 `[-1, 1]` 的 `f32` 采样。游戏声音只能连接一次，连接后不得再调用离屏混音；会话退出时由宿主停止回调并释放混音器。

宿主可通过 `host::audio::SpeechSynthesizer` 提供合成的 `Pcm`；字体服务及请求、编码和字形遮罩类型从 `host::font` 导入。遮罩每个像素提供一个 `0..=64` 的覆盖值，按从顶行开始的顺序排列。可复用 `message::Fonts` 处理宿主提供的字体，或实现自己的服务。软件图形后端、字形实现与混音器都可在无设备环境运行。

装载电影需要宿主先通过 `set_movie_decoder_factory` 注册 `host::movie::MovieDecoderFactory`。工厂打开 `SharedSource`，解码器返回 BGRA 帧与交错 PCM。更换工厂不影响已装载电影。默认库构建无需 C 工具链；启用 `builtin-mpeg` 并注册 `movie::MpegDecoderFactory` 可解码 MPEG-1 视频及 MP2／MP3 音轨。

角色模型由 `set_model_services` 注册的宿主负责。会话已绑定资源服务时，`ModelServices::open_with_resources` 同时接收 FES 模型选择的原始字节和会话的资源解析器，可沿用同一归档、补丁和散装资源查找规则。自行管理资源的宿主可实现 `ModelServices::open`。 宿主可用 `ModelSelection::parse` 和 `ModelSelection::open` 取得主体 PSZ 的读取源、初始动作名与选中动作行的差分表名称。选择中的后续选项仍保留原始字节。主体和初始动作相同的选择可通过 `ModelPlayer::set_options` 更新参数并延续播放。返回 `false` 时会话重新打开播放器，返回错误时保留请求供重试。`host::model::ModelConfig` 解析 `m2config` 类别中的主体及动作表，保留主体别名和空动作字段。主体资源可经 `m2psb` 类别解析，`decode_psz` 按宿主指定的解压字节上限提取 PSB 数据。

FES 的 `m2body` 指令通过 `ModelPlayer::set_body_options` 交给当前播放器。同一更新中的指令按脚本顺序在取帧前执行，错误时保留尚未执行的指令。`m2enable_eff` 通过 `ModelPlayer::set_effects_mode` 提交模型平面的效果值。默认值为 `1`，显式设置会保留完整 DWORD，并在播放器替换后重新应用。

运行库按会话时钟向可见对象的播放实例请求透明 BGRA 帧，并把帧及锚点偏移交给合成器。偏移用平面变换前的逻辑像素表示。服务缺失或返回 `None` 时，脚本继续推进，模型平面无可见内容。删除平面或释放场景会释放播放实例。

示例宿主可通过 `KATZE_MODEL_HOST` 指定一个实现以下流协议的模型播放器。每个模型实例启动一个进程，使用标准输入和标准输出通信，诊断使用标准错误。全部整数使用小端序。字段为 `u32` 字节数和紧随其后的数据，空字段长度为零。

|消息|主程序发出|播放器回应|
|:--|:--|:--|
|打开|`KMOD` 加版本字节 `1`，随后依次发送 PSZ、初始动作名、表情差分表、主体差分表和选择选项五个字段。|`Y` 接受，`B` 同时支持主体差分，`E` 同时支持主体差分与效果值，`N` 表示不可用。|
|参数更新|`O` 加一个选项字段。|`Y` 接受，`N` 请求重新打开。|
|主体差分|打开回应为 `B` 或 `E` 时发送。`B` 加一个主体选项字段。|`Y` 接受，`N` 返回错误。|
|模型效果|打开回应为 `E` 时发送。`E` 加 `u32` 效果值。|`Y` 接受，`N` 返回错误。|
|取帧|`F` 加 `u32` 会话毫秒数。|`F` 加 `u32` 宽高、`i32` 锚点偏移 X/Y 以及紧密排列的 BGRA8 数据。`N` 隐藏并清空当前帧，`R` 继续使用上一份可见帧。|

示例限制单个 PSZ 为 128 MiB、各差分表为 4 MiB、返回帧为 64 MiB。播放器应刷新每条回应。释放模型时示例终止并回收对应进程。

离屏和桌面示例可设置 `KATZE_FFMPEG_MOVIES=1`，让示例使用 `PATH` 中的 `ffprobe` 和 `ffmpeg` 探测并解码电影，支持 VP8／Vorbis WebM。此模式要求媒体能够从不可寻址的输入管道解码。视频按报告的帧率输出，音轨输出同一时间基准下的立体声 PCM。电影资源通过宿主提供的流读取，示例无需把电影解包到磁盘。运行库不需要外部媒体程序。

宿主在更新后可用 `take_script_messages` 取走调试文字。即使更新返回错误，已生成的消息仍可取出。`set_native_trace` 注册当前运行实例的可选回调，运行时分派 native 时发送开始和结束事件，带任务号、原始调用编号、VM 字节偏移及失败标记。调用可嵌套，计时和输出由宿主决定。两个示例使用 `KATZE_TRACE_SCRIPT` 显示脚本消息，使用 `KATZE_TRACE_TIMING` 记录慢调用。

宿主可通过 `script_positions` 只读枚举已登记脚本的当前位置，包括失败后仍登记的脚本。KCS 报告字节偏移；FES 报告块、块内指令索引和是否有待续处理的指令。失败位置以 `advance` 返回的错误为准。

`set_diagnostic_trace` 为当前实例启用指定类别的诊断，例如输入、任务消息及 FES。回调在推进过程中同步收到类别和借用的格式化参数；如需保留请在回调中转成自有字符串。诊断文字可随版本变化，按编号统计 native 调用应使用结构化的 `set_native_trace`。传入 `None` 关闭探针。示例可设置 `KATZE_TRACE_MOVIE` 查看独占电影句柄的装载、播放、停止及释放；时间字段使用宿主传入的 `system_time_ms`。

默认工作区包 `katze-system2` 发布为库。离屏宿主入口是 `cargo run --release --example runprobe -- "$GAME_DIR"`；需要 MPEG 解码时加入 `--features builtin-mpeg`，或使用上述 FFmpeg 模式。普通运行直接使用 `GameRuntime`，可用 `KATZE_PROBE_SHOT` 输出终帧、用 `KATZE_PROBE_AUDIO_STATS=1` 统计内存 PCM。 `KATZE_PROBE_SOUND=0,1` 记录指定物理声道的播放、音量变化和停止，`all` 记录所有声道。 `KATZE_PROBE_DETACHED_MIXER=1` 使用与设备回调相同的独立混音器，将输出写入内存。同一示例的自动回放和 VM 检查由辅助模块通过任务接口执行。 设置 `KATZE_PROBE_SHARED` 为 `起始槽:数量` 可在 `KATZE_PROBE_DUMP` 指定的轮次查看连续的共享数值槽。使用 `KATZE_PROBE_NATURAL_MOVIES=1` 时，自动点击和 Ctrl 快进在独占电影装载后暂停，直到脚本释放电影。脚本检查工具和桌面宿主是单独的仓库内包，分别通过 `cargo run -p katze-script -- ...` 和 `cargo run -p katze-desktop --bin window_startup -- "$GAME_DIR"` 执行。桌面宿主默认启用 MPEG 解码，桌面输入的合成验证用 `cargo test -p katze-desktop --test window_host`。

标准会话默认在调用线程解码。Vorbis 装载时准备完整 PCM，每个来源受 64 MiB 的 PCM 容量上限约束。电影随会话推进增量解码，内存混音在需要更多样本时也可以推进解码。设备回调只取已准备的 PCM。宿主可在启动前用 `set_media_decode_mode(DecodeMode::Threaded)` 开启后台电影解码和 Vorbis 预取。两个示例宿主均启用此模式。`ReadSource::read_at` 可能在后台调用，读取源须支持独立偏移与并发读取。

文件导出服务在会话线程选择目的地。`ExportServices::start` 默认同步执行 `ExportWork`。宿主也可以自行安排工作并返回 `ExportJob`，由会话轮询完成状态。完成通知仍在会话线程回调，导出任务销毁时会取消未执行的工作。
