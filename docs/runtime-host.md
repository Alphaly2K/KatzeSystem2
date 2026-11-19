# 运行库与宿主边界

KatzeSystem2 提供 CatSystem2 2.6.1.90 的 Rust 运行库。宿主创建实例，提供输入、时间和服务，推进运行并消费输出。参考 art3m1s-core 的会话、资源提供者和绘制列表边界，沿用本项目已有脚本行为、资产格式和运行状态。

|边界|运行库|宿主|
|:--|:--|:--|
|资源与持久化|解析归档和脚本、解释资源名称、编解码存档，通过读取源及逻辑操作访问数据。|选择目录或其他存储，实现资源、配置、存档、导出和脚本文件服务；处理文件路径、枚举、权限及修改时间。|
|画面|合成场景、生成有序绘制命令、记录纹理来源和尺寸；保留脚本内部像素操作与截图语义。|执行绘制和窗口呈现，持有 GPU 资源；按脚本所需提供离屏回读。现有软件绘制可作为示例和测试后端。|
|声音与电影|解码、混音、控制循环与音量，提供 PCM 及电影帧；保留任务查询和完成时序。|选择音频设备、流格式和缓冲，拉取 PCM 并呈现电影；可选提供语音合成。|
|网络与系统|按脚本协议分发请求并管理任务状态。类型 62 的旧版社交发帖接收者暂未接入。|执行网络或系统动作，实施策略，回交结果；处理窗口、剪贴板、输入法、插件及对话框。|
|输入、字体和时钟|处理游戏输入和逻辑时钟，按字体数据完成字形度量与栅格化。|提供输入事件、运行时间和当地日期时间，发现字体并提供数据。|

嵌入时从 `katze_system2::runtime` 导入会话类型，从 `katze_system2::host` 导入服务契约及输入、呈现和 PCM 类型。任务树、原始命令编号、VM 缓冲区和分派表属于底层工具入口。现有服务类型的模块路径仍可使用。Rust 契约稳定且有非 Rust 宿主接入时，再定义独立版本的 C ABI 和显式释放约定。

标准宿主用 `GameRuntime::with_programs(loader, shared_slots, external)` 创建根任务并配置共享存储。启动前可将启动文档、游戏默认值及用户设置的 XML 字节交给 `configure_startup`，依次应用引擎配置；无效文档保留原有状态。绑定输入、资源和需要的其他服务，调用 `configure_canvas([width, height])` 设置逻辑输出尺寸并按游戏配置创建画布，再调用 `start_game(entry)`。该方法消费准备好的实例，返回已启动的实例。缺失的入口名称返回诊断并释放本次实例。脚本工具可通过 `start_game_with_tasks(entry)` 另外取得用于诊断的 `GameTasks`。

标准会话在推进前必须通过 `set_input_device` 绑定 `InputDevice`。离屏宿主可提供只返回空闲状态的内存设备。`startup_input_bindings` 返回已经应用用户覆盖的八十个逻辑键槽，宿主可以用它构造 `DeviceKeys`。配置保存优先读取设备当前的绑定；设备未提供绑定时使用启动配置。

`GameRuntime::set_game_resources` 接受资源服务与声音配置文档。运行库应用声音定义和用户覆盖，再按文件前缀选定归档类别；宿主按该类别补全名称并提供读取源。没有定义时使用启动文档里的声音组。自定义声音路由的宿主可以用底层 `Runtime::set_resources`，由其资源服务直接处理 `sound` 类别。

调用 `advance(FrameTime { tick, increment, system_time_ms })` 推进一轮。`tick` 为游戏逻辑时钟（通常每秒 60 个单位），`increment` 是本轮传给任务的逻辑增量，`system_time_ms` 为可回绕的系统毫秒计数。先处理上一轮语言请求，再推进媒体和任务。语言刷新失败时保留失败项及后续请求，已成功应用的界面刷新在下一次成功推进时报告。

`FrameOutcome::Running { interface_language_refreshed }` 表示本轮成功。标志为真时，窗口宿主可以通过 `string_tables` 更新界面译文，再消费绘制和声音输出。`FrameOutcome::Exited` 表示根任务已退出，应结束会话循环；再次调用 `advance` 仍返回 `Exited`。需要自定义结构的调用方仍可用 `Runtime::new` 和 `update`。

窗口宿主在更新后调用 `process_window_requests(window)` 执行脚本提交的窗口请求。设备操作由 `WindowServices` 提供，运行库负责配置回写和任务通知。返回 true 表示应退出窗口循环；发生错误时可在后续调用继续处理尚未尝试的请求。文本对话框通过 `text_entry_request` 取得初始文字及请求句柄，使用同一个句柄调用 `complete_text_entry` 确认或取消。以 `Arc::ptr_eq` 区分请求；同样的初始文字可以属于不同请求。过期请求返回 false。

宿主通过 `set_archive_config`、`set_folder_paths`、`set_script_settings` 和 `set_platform_settings` 提供脚本可查的路径及初始配置。启动配置和资源服务就绪后调用 `load_startup_language_tables` 装入初始译文；没有可用的界面语言前缀时，仅装入游戏表。

`set_surface_size` 提供逻辑画面尺寸；窗口宿主可独立通过 `set_window_content_rect` 和 `set_windowed_mode` 提供客户区绘图矩形和当前窗口模式。`take_clipboard_requests` 取走按序提交的剪贴板写入请求，保留发起任务和原始编码，由宿主解码并写入剪贴板。

宿主用 `Runtime::with_frame` 借用绘制命令和画布。`wants_sequence_capture` 为真时，宿主读回同一帧，再用 `capture_sequence_bgra` 同步提交从顶行开始的 BGRA 像素和以字节计的行距。运行库负责裁剪、编码 BMP 和编号，通过宿主文件服务写入。

离屏宿主在更新后用 `mix_audio_stereo(rate_hz, buffer)` 拉取 PCM；此调用可等待解码。设备宿主用 `connect_audio_output()` 取得可移交给音频线程的 `Mixer`，由回调用 `Mixer::render(rate_hz, buffer)` 拉取 PCM，无须借用运行实例。两者均覆写调用方缓冲，格式为左右交错、幅度 `[-1, 1]` 的 `f32` 采样。游戏声音只能连接一次，连接后不得再调用离屏混音；会话退出时由宿主停止回调并释放混音器。

宿主在更新后可用 `take_script_messages` 取走调试文字。即使更新返回错误，已生成的消息仍可取出。`set_native_trace` 注册当前运行实例的可选回调，运行时分派 native 时发送开始和结束事件，带任务号、原始调用编号、VM 字节偏移及失败标记。调用可嵌套，计时和输出由宿主决定。两个示例使用 `KATZE_TRACE_SCRIPT` 显示脚本消息，使用 `KATZE_TRACE_TIMING` 记录慢调用。

`set_diagnostic_trace` 为当前实例启用指定类别的诊断，例如输入、任务消息及 FES。回调在推进过程中同步收到类别和借用的格式化参数；如需保留请在回调中转成自有字符串。诊断文字可随版本变化，按编号统计 native 调用应使用结构化的 `set_native_trace`。传入 `None` 关闭探针。示例可设置 `KATZE_TRACE_MOVIE` 查看独占电影句柄的装载、播放、停止及释放；时间字段使用宿主传入的 `system_time_ms`。

默认工作区包 `katze-system2` 发布为库。离屏宿主入口是 `cargo run --release --example runprobe -- "$GAME_DIR"`。脚本检查工具和桌面宿主是单独的仓库内包，分别通过 `cargo run -p katze-script -- ...` 和 `cargo run -p katze-desktop --bin window_startup -- "$GAME_DIR"` 执行。桌面输入的合成验证用 `cargo test -p katze-desktop --test window_host`。

迁移按下列顺序进行：

1. 宿主服务统一从 `host` 导出，两份示例通过有类型的启动配置和语言查询接入。文件通过 `ReadSource`、`ResourceServices`、`SaveServices`、`FileServices` 访问；当地时间通过 `SystemServices::local_date_time` 提供；字体发现和私有字体读取由宿主提供。
2. 合成器通过 `Runtime::with_frame` 借出绘制列表和画布，由宿主呈现；库内 `GraphicsBackend` 用于脚本离屏回读。Metal、窗口和音频设备均由示例宿主管理。
3. 为已有系统路由明确请求、返回与取消的对应关系。同步查询保持同步；需要宿主等待的操作沿用任务推进和轮询。社交发帖协议随[兼容项](compatibility-gaps.md)延期。诊断信息通过公开入口交给宿主。编解码和计算路径的线程、时钟及工具链要求单独检查，在宿主确有需要时提供相应构建路径。
4. `runprobe` 负责本地资源和隔离存档，消费绘制列表和内存 PCM。桌面窗口和设备输出使用独立的可选示例适配。用内存宿主回归验证公开 API，再用 `runprobe` 验证两款游戏的剧情、存档与媒体行为。

发布检查覆盖独立库构建、正常及构建依赖树、内存宿主用例和原有离屏流程。SDL 和 Metal 仅由桌面宿主构建。`runprobe` 仍使用系统字体发现及当地时间服务；这两项是运行库的开发依赖，不会进入外部宿主的依赖树。
