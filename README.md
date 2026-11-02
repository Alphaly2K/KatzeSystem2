# KatzeSystem2

CatSystem2 的开源重实现，当前适配基线为 Windows CatSystem2 2.6.1.90。现阶段提供 Rust 脚本解释库、图像解码、图形与文字基础接口和脚本检查工具，尚未提供完整游戏播放器。

需要 Rust 1.87 或更新版本。

```sh
cargo test --locked
cargo run --bin katze-script -- eval 2048 '1+2*3'
cargo run --bin katze-script -- inspect demo.cst
```

`inspect` 接受一个或多个文件，也可检查目录中的文件；输出读取数量与结构诊断数量。

[脚本参考](docs/README.md) · [接口使用与实现范围](docs/api.md) · [阶段进展](GOAL.md) · [依赖许可证](THIRD_PARTY.md)

项目采用 [LGPL-2.1](LICENSE)。
