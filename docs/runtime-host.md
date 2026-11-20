# 运行库与宿主边界

KatzeSystem2 提供 CatSystem2 2.6.1.90 的 Rust 运行库。宿主创建实例，提供输入、时间和服务，推进运行并消费输出。参考 art3m1s-core 的会话、资源提供者和绘制列表边界，沿用本项目已有脚本行为、资产格式和运行状态。

|边界|运行库|宿主|
|:--|:--|:--|
|资源与持久化|解析归档和脚本、解释资源名称、编解码存档，通过读取源及逻辑操作访问数据。|选择目录或其他存储，实现资源、配置、存档、导出和脚本文件服务；处理文件路径、枚举、权限及修改时间。|
|画面|合成场景、生成有序绘制命令、记录纹理来源和尺寸；保留脚本内部像素操作与截图语义。|执行绘制和窗口呈现，持有 GPU 资源；按脚本所需提供离屏回读。现有软件绘制可作为示例和测试后端。|
|声音与电影|解码、混音、控制循环与音量，提供 PCM 及电影帧；保留任务查询和完成时序。|选择音频设备、流格式和缓冲，拉取 PCM 并呈现电影；可选提供语音合成。|
|网络与系统|按脚本协议分发请求并管理任务状态。类型 62 的旧版社交发帖接收者暂未接入。|执行网络或系统动作，实施策略，回交结果；处理窗口、剪贴板、输入法、插件及对话框。|
|输入、字体和时钟|处理游戏输入和逻辑时钟，按字体数据完成字形度量与栅格化。|提供输入事件、运行时间和当地日期时间，发现字体并提供数据。|

嵌入时从 `katze_system2::session` 导入 `GameRuntime`、`FrameTime` 和 `FrameOutcome`，从 `katze_system2::host` 导入服务契约及输入、绘制命令、纹理、PCM 和诊断类型。`GameRuntime` 是具体的会话类型。任务树、原始命令编号、VM 缓冲区和分派表在底层 `runtime` 接口中供诊断和工具使用。Rust 契约稳定且有非 Rust 宿主接入时，再定义独立版本的 C ABI 和显式释放约定。

标准宿主用 `GameRuntime::prepare(shared_slots, external_bytes)` 创建根任务和清零后的脚本存储。前者按 32 位值计数，后者按字节计数。启动前可将启动文档、游戏默认值及用户设置的 XML 字节交给 `configure_startup`，由它应用引擎与平台配置并返回游戏逻辑尺寸；配置无效时保留原有状态。绑定输入、资源和需要的其他服务，调用 `configure_canvas` 设置非零的逻辑输出尺寸并按游戏配置创建画布，再把启动配置中的脚本名称交给 `start_game(entry)`。该方法消费准备好的实例，并在创建任务前检查资源、输入和画布。宿主可先调用 `validate_startup` 检查这些条件；该检查不会访问宿主服务或装载脚本。两个方法使用 `session::StartupError` 分别报告缺少的服务；任务创建或入口装载失败由 `StartTasks` 携带原始诊断。

逻辑尺寸取启动文档的 `SCREEN/width` 和 `SCREEN/height`。宿主通过 `window_preferences` 读取用户窗口尺寸、启动文档的全屏候选尺寸及当前全屏偏好，再根据设备能力决定实际模式。`configure_canvas` 同时设置呈现和输入使用的逻辑尺寸。桌面宿主按实际窗口尺寸换算鼠标坐标；离屏宿主的显式回放点击使用逻辑坐标，截图保持逻辑尺寸。

普通 KCS 的入口和后续装载使用已绑定的 `ResourceServices`。运行库按 `kcs` 类别补全名称，以 `ArchiveFirst` 模式读取字节并解析；补丁与归档顺序由资源服务决定。工具可用 `GameRuntime::into_task_runtime` 接续同一个实例，也可直接使用 `runtime::TaskRuntime::with_programs` 提供预解析程序，或用 `TaskRuntime::with_resource_programs` 绑定现有外部存储。`TaskRuntime::start_game_with_tasks(entry)` 可另外取得用于诊断的任务编号。

宿主通过 `ResourceServices` 提供资源，使用 `FileServices`、`SaveServices` 和 `ConfigurationServices` 分别接入脚本文件、存档和配置持久化。资源服务返回完整字节或可独立读取的 `SharedSource`；宿主可用 `CachedResources` 为自定义装载流程复用预读缓存。这些契约统一从 `host` 导入；旧的 `io`、`save` 和 `settings` 导入路径仍可用。

标准会话在启动前必须通过 `set_input_device` 绑定 `host::InputDevice`。离屏宿主可提供只返回空闲状态的内存设备。`startup_input_bindings` 返回已经应用用户覆盖的八十个逻辑键槽，宿主可以用它构造 `DeviceKeys` 或自行维护输入状态。配置保存用 `InputDevice::key_configuration` 借用 `&[DeviceBinding; 80]`；使用 `DeviceKeys` 的宿主可返回 `Some(keys.bindings())`。返回 `None` 时使用启动配置。

`GameRuntime::set_game_resources` 接受资源服务与声音配置文档。运行库按声音定义选定归档类别，应用用户覆盖以控制播放；宿主按该类别补全名称并提供读取源。没有新版声音定义时，使用启动文档里的旧版文件映射和声音组。自定义声音路由的宿主可以用 `set_resources`，由其资源服务直接处理 `sound` 类别。

调用 `advance(FrameTime { tick, increment, system_time_ms })` 推进一轮。`tick` 为游戏逻辑时钟（通常每秒 60 个单位），`increment` 是本轮传给任务的逻辑增量，`system_time_ms` 为可回绕的系统毫秒计数。先处理上一轮语言请求，再推进媒体和任务。语言刷新失败时保留失败项及后续请求，已成功应用的界面刷新在下一次成功推进时报告。

`GameRuntime::advance` 使用 `session::AdvanceError` 区分语言、电影、任务和配置通知错误，`diagnostic()` 返回原始诊断。`LanguageRefresh` 可在资源恢复后重新尝试，失败的调用尚未推进任务。`ConfigurationNotification` 会在下一次完成语言和电影阶段后继续通知已推进轮次的剩余对象。`TaskUpdate` 可能已经改动状态，宿主应按诊断处理。任务工具的 `Runtime::advance` 和 `update` 仍返回原始 `Error`。

`FrameOutcome::Running { interface_language_refreshed }` 表示本轮成功。标志为真时，窗口宿主可以通过 `string_tables` 更新界面译文，再消费绘制和声音输出。`FrameOutcome::Exited` 表示根任务已退出，应结束会话循环；再次调用 `advance` 仍返回 `Exited`。需要自定义结构的调用方仍可用 `Runtime::new` 和 `update`。

窗口宿主在更新后调用 `process_window_requests(window)` 执行脚本提交的窗口请求。设备操作由 `WindowServices` 提供，运行库负责配置回写和任务通知。返回 true 表示应退出窗口循环；发生错误时可在后续调用继续处理尚未尝试的请求。文本对话框通过 `text_entry_request` 取得初始文字及请求句柄，使用同一个句柄调用 `complete_text_entry` 确认或取消。以 `Arc::ptr_eq` 区分请求；同样的初始文字可以属于不同请求。过期请求返回 false。

宿主通过 `set_archive_config`、`set_folder_paths` 和 `set_script_settings` 提供脚本可查的路径及初始配置。图片导入宿主可用 `image_import_directory` 读取、用 `set_image_import_directory` 更新最后选择的 UTF-16 路径，供随后保存配置。标准会话的窗口和目录接口取代直接借用或替换原始设置；任务工具仍可从 `settings` 访问原始设置类型。启动配置和资源服务就绪后调用 `load_startup_language_tables` 装入初始译文；没有可用的界面语言前缀时，仅装入游戏表。

窗口宿主可通过 `set_window_content_rect` 和 `set_windowed_mode` 提供客户区绘图矩形和当前窗口模式。`take_clipboard_requests` 取走按序提交的剪贴板写入请求，保留发起任务和原始编码，由宿主解码并写入剪贴板。

宿主用 `GameRuntime::present_frame` 借用当前绘制列表和 `FrameCanvas`。通过 `texture` 解析命令中的纹理，通过 `tiles` 和 `dirty_tiles` 更新画布缓存。呈现和脚本回读共用一个纹理缓存时，由宿主上传所有变更面块后调用 `acknowledge_upload`；有独立缓存的多个消费者应各自跟踪更新。

`set_graphics_backend` 设置脚本同步离屏回读使用的后端。`GraphicsBackend::render` 在透明背景上执行命令，返回与请求尺寸一致、从顶行开始紧密排列的 BGRA8 `Surface`。旧实现若返回单块 `RenderTarget`，可直接返回该块的像素面。未设置时使用库内的软件后端；软件宿主若复用已有目标，可用 `SoftwareBackend::render_into`。

全屏转场的活动状态随 `advance` 更新。宿主可跳过或重复取帧，脚本无需等待呈现便能收到转场完成状态。

需要连续截图时，回调同时收到一个 `SequenceCapture`。呈现提交后可通过它交回软件渲染目标，或交回从顶行开始的 BGRA 像素及以字节计的行距。每次请求只能提交一次；跳过呈现时可丢弃请求。运行库负责裁剪、编码 BMP 和编号，通过宿主文件服务写入。只需查看或离屏绘制时可用 `with_frame`。

离屏宿主在更新后用 `mix_audio_stereo(rate_hz, buffer)` 拉取 PCM；此调用可等待解码。设备宿主用 `connect_audio_output()` 取得可移交给音频线程的 `Mixer`，由回调用 `Mixer::render(rate_hz, buffer)` 拉取 PCM，无须借用运行实例。两者均覆写调用方缓冲，格式为左右交错、幅度 `[-1, 1]` 的 `f32` 采样。游戏声音只能连接一次，连接后不得再调用离屏混音；会话退出时由宿主停止回调并释放混音器。

宿主可通过 `host::SpeechSynthesizer` 提供合成的 `Pcm`；字体服务从 `host` 导入 `FontServices`、`FontRequest` 和 `GlyphMask`。可复用 `message::Fonts` 处理宿主提供的字体，或实现自己的服务。软件图形后端、字形实现与混音器都可在无设备环境运行。

宿主在更新后可用 `take_script_messages` 取走调试文字。即使更新返回错误，已生成的消息仍可取出。`set_native_trace` 注册当前运行实例的可选回调，运行时分派 native 时发送开始和结束事件，带任务号、原始调用编号、VM 字节偏移及失败标记。调用可嵌套，计时和输出由宿主决定。两个示例使用 `KATZE_TRACE_SCRIPT` 显示脚本消息，使用 `KATZE_TRACE_TIMING` 记录慢调用。

宿主可通过 `script_positions` 只读枚举已登记脚本的当前位置，包括失败后仍登记的脚本。KCS 报告字节偏移；FES 报告块、块内指令索引和是否有待续处理的指令。失败位置以 `advance` 返回的错误为准。

`set_diagnostic_trace` 为当前实例启用指定类别的诊断，例如输入、任务消息及 FES。回调在推进过程中同步收到类别和借用的格式化参数；如需保留请在回调中转成自有字符串。诊断文字可随版本变化，按编号统计 native 调用应使用结构化的 `set_native_trace`。传入 `None` 关闭探针。示例可设置 `KATZE_TRACE_MOVIE` 查看独占电影句柄的装载、播放、停止及释放；时间字段使用宿主传入的 `system_time_ms`。

默认工作区包 `katze-system2` 发布为库。离屏宿主入口是 `cargo run --release --example runprobe -- "$GAME_DIR"`。普通运行直接使用 `GameRuntime`，可用 `KATZE_PROBE_SHOT` 输出终帧、用 `KATZE_PROBE_AUDIO_STATS=1` 统计内存 PCM。自动选项回放和 VM 检查通过同一示例的任务接口执行。脚本检查工具和桌面宿主是单独的仓库内包，分别通过 `cargo run -p katze-script -- ...` 和 `cargo run -p katze-desktop --bin window_startup -- "$GAME_DIR"` 执行。桌面输入的合成验证用 `cargo test -p katze-desktop --test window_host`。

库依赖树不含 SDL、Metal、系统字体发现或当地时间库。当前电影解码构建使用 C 编译器，流式音频解码使用工作线程；在不提供这些能力的目标上须单独评估。`ReadSource::read_at` 可能在解码线程调用，宿主须提供可共享的独立读取源。
