# 第三方依赖

项目自身采用 LGPL-2.1。当前依赖及其传递依赖选择以下许可证，均可与本项目许可证一同使用。分发时须保留对应版权及许可声明；`encoding_rs` 包含 WHATWG 的 BSD-3-Clause 声明，`ogg` 采用 BSD-3-Clause，二进制分发材料也须保留这些声明。

|包|版本|采用的许可|声明|
|:--|:--|:--|:--|
|flate2|1.1.9|MIT|[许可](licenses/flate2-1.1.9/LICENSE-MIT)|
|encoding_rs|0.8.35|MIT + BSD-3-Clause|[MIT](licenses/encoding_rs-0.8.35/LICENSE-MIT)、[WHATWG](licenses/encoding_rs-0.8.35/LICENSE-WHATWG)、[版权](licenses/encoding_rs-0.8.35/COPYRIGHT)|
|miniz_oxide|0.8.9|Zlib|[许可](licenses/miniz_oxide-0.8.9/LICENSE-ZLIB.md)|
|crc32fast|1.5.3|MIT|[许可](licenses/crc32fast-1.5.3/LICENSE-MIT)|
|cfg-if|1.0.5|MIT|[许可](licenses/cfg-if-1.0.5/LICENSE-MIT)|
|adler2|2.0.1|MIT|[许可](licenses/adler2-2.0.1/LICENSE-MIT)|
|simd-adler32|0.3.10|MIT|[许可](licenses/simd-adler32-0.3.10/LICENSE.md)|
|roxmltree|0.20.0|MIT|[许可](licenses/roxmltree-0.20.0/LICENSE-MIT)|
|fontdue|0.9.3|MIT|[许可](licenses/fontdue-0.9.3/LICENSE-MIT)|
|ttf-parser|0.21.1|MIT|[许可](licenses/ttf-parser-0.21.1/LICENSE-MIT)|
|lewton|0.10.2|MIT|[许可](licenses/lewton-0.10.2/LICENSE)|
|ogg|0.8.0|BSD-3-Clause|[许可](licenses/ogg-0.8.0/LICENSE)|
|tinyvec|1.13.3|MIT|[许可](licenses/tinyvec-1.13.3/LICENSE-MIT.md)|
|blowfish|0.9.1|MIT|[许可](licenses/blowfish-0.9.1/LICENSE-MIT)|
|cipher|0.4.4|MIT|[许可](licenses/cipher-0.4.4/LICENSE-MIT)|
|inout|0.1.4|MIT|[许可](licenses/inout-0.1.4/LICENSE-MIT)|
|byteorder|1.5.0|MIT|[许可](licenses/byteorder-1.5.0/LICENSE-MIT)|
|crypto-common|0.1.7|MIT|[许可](licenses/crypto-common-0.1.7/LICENSE-MIT)|
|generic-array|0.14.7|MIT|[许可](licenses/generic-array-0.14.7/LICENSE)|
|typenum|1.20.1|MIT|[许可](licenses/typenum-1.20.1/LICENSE-MIT)|
|version_check|0.9.5|MIT|[许可](licenses/version_check-0.9.5/LICENSE-MIT)|

依赖版本与校验值由 `Cargo.lock` 固定。Zlib 解压采用 Rust 后端，不要求系统 Zlib。未引入先行实现的源码或资源。
