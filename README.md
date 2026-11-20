# KatzeSystem2

CatSystem2 2.6.1.90 的可嵌入 Rust 运行库。宿主提供资源、输入和设备服务；仓库另提供离屏运行示例及脚本检查工具。

需要 Rust 1.87 或更新版本；默认的电影解码功能还需要 C 编译器。嵌入时可关闭 `builtin-mpeg` 并接入宿主解码器。

```sh
cargo test --locked
cargo run --locked --release --example runprobe -- "$GAME_DIR"
cargo run --locked -p katze-script -- eval 2048 '1+2*3'
cargo run --locked -p katze-script -- inspect demo.cst
cargo run --locked -p katze-script -- native-calls ordinary scripts/
```

`inspect` 接受一个或多个文件，也可检查目录中的文件；输出读取数量与结构诊断数量。

`native-calls` 按指定宿主表批量统计 KCS 调用编号、参数字节数和调用点数量。

桌面宿主单独构建：`cargo run --locked --release -p katze-desktop --bin window_startup -- "$GAME_DIR"`。

有 WebM 电影的游戏可设置 `KATZE_FFMPEG_MOVIES=1`，并确保 `ffmpeg` 和 `ffprobe` 在 `PATH` 上。

[嵌入运行库](docs/runtime-host.md) · [脚本参考](docs/README.md) · [接口使用与实现范围](docs/api.md) · [阶段进展](GOAL.md) · [依赖许可证](THIRD_PARTY.md)

项目采用 [LGPL-2.1](LICENSE)。
