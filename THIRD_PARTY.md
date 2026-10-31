# 第三方依赖

项目自身采用 LGPL-2.1。当前依赖及其传递依赖选择以下许可证，均可与本项目许可证一同使用。分发时须保留对应版权及许可声明；`encoding_rs` 还包含 WHATWG 的 BSD-3-Clause 声明，二进制分发材料也须保留该声明。

|包|版本|采用的许可|声明|
|:--|:--|:--|:--|
|flate2|1.1.9|MIT|[许可](licenses/flate2-1.1.9/LICENSE-MIT)|
|encoding_rs|0.8.35|MIT + BSD-3-Clause|[MIT](licenses/encoding_rs-0.8.35/LICENSE-MIT)、[WHATWG](licenses/encoding_rs-0.8.35/LICENSE-WHATWG)、[版权](licenses/encoding_rs-0.8.35/COPYRIGHT)|
|miniz_oxide|0.8.9|Zlib|[许可](licenses/miniz_oxide-0.8.9/LICENSE-ZLIB.md)|
|crc32fast|1.5.3|MIT|[许可](licenses/crc32fast-1.5.3/LICENSE-MIT)|
|cfg-if|1.0.5|MIT|[许可](licenses/cfg-if-1.0.5/LICENSE-MIT)|
|adler2|2.0.1|MIT|[许可](licenses/adler2-2.0.1/LICENSE-MIT)|
|simd-adler32|0.3.10|MIT|[许可](licenses/simd-adler32-0.3.10/LICENSE.md)|

依赖版本与校验值由 `Cargo.lock` 固定。Zlib 解压采用 Rust 后端，不要求系统 Zlib。未引入先行实现的源码或资源。
