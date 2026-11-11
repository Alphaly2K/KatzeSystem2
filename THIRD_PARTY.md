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
|fontdb|0.23.0|MIT|[许可](licenses/fontdb-0.23.0/LICENSE)|
|fontconfig-parser|0.5.8|MIT|[许可](licenses/fontconfig-parser-0.5.8/LICENSE)|
|ttf-parser|0.21.1|MIT|[许可](licenses/ttf-parser-0.21.1/LICENSE-MIT)|
|ttf-parser|0.25.1|MIT|[许可](licenses/ttf-parser-0.25.1/LICENSE-MIT)|
|slotmap|1.1.1|Zlib|[许可](licenses/slotmap-1.1.1/LICENSE)|
|core_maths|0.1.1|MIT|[许可](licenses/core_maths-0.1.1/LICENSE)|
|libm|0.2.16|MIT|[许可及上游声明](licenses/libm-0.2.16/LICENSE.txt)|
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

依赖版本与校验值由 `Cargo.lock` 固定。Zlib 解压采用 Rust 后端，不要求系统 Zlib。字体服务使用 fontdb 统一发现、fontdue 栅格化和 ttf-parser 元数据解析；fontdb 启用 fs／fontconfig，fontconfig-parser 使用纯 Rust 解析配置，关闭 memmap。新增依赖采用上表的 MIT 或 Zlib 许可，分发时保留许可及版权声明；slotmap 的来源与修改标记义务按其 Zlib 许可执行。未引入先行实现的源码或资源。

启动示例使用开发依赖 `libc` 0.2.191 调用平台本地日期转换，选择 MIT 许可，兼容项目 LGPL-2.1；分发示例时保留其[版权及许可声明](licenses/libc-0.2.191/LICENSE-MIT)。

macOS 图形后端使用 Metal 的 Rust 绑定。以下依赖选择 MIT 分支；`unicode-ident` 另保留 Unicode-3.0 声明，分发时随附全部版权和许可文字。`block`、`malloc_buf` 的发布包通过包元数据声明 MIT，本项目同时保留声明的作者及 MIT 正文。

|包|版本|采用的许可|声明|
|:--|:--|:--|:--|
|bitflags|1.3.2|MIT|[声明](licenses/bitflags-1.3.2)|
|bitflags|2.13.2|MIT|[声明](licenses/bitflags-2.13.2)|
|block|0.1.6|MIT|[声明](licenses/block-0.1.6)|
|core-foundation|0.9.4|MIT|[声明](licenses/core-foundation-0.9.4)|
|core-foundation-sys|0.8.7|MIT|[声明](licenses/core-foundation-sys-0.8.7)|
|core-graphics-types|0.1.3|MIT|[声明](licenses/core-graphics-types-0.1.3)|
|foreign-types|0.5.0|MIT|[声明](licenses/foreign-types-0.5.0)|
|foreign-types-macros|0.2.4|MIT|[声明](licenses/foreign-types-macros-0.2.4)|
|foreign-types-shared|0.3.1|MIT|[声明](licenses/foreign-types-shared-0.3.1)|
|log|0.4.35|MIT|[声明](licenses/log-0.4.35)|
|malloc_buf|0.0.6|MIT|[声明](licenses/malloc_buf-0.0.6)|
|metal|0.28.0|MIT|[声明](licenses/metal-0.28.0)|
|objc|0.2.7|MIT|[声明](licenses/objc-0.2.7)|
|paste|1.0.15|MIT|[声明](licenses/paste-1.0.15)|
|proc-macro2|1.0.107|MIT|[声明](licenses/proc-macro2-1.0.107)|
|quote|1.0.47|MIT|[声明](licenses/quote-1.0.47)|
|syn|3.0.9|MIT|[声明](licenses/syn-3.0.9)|
|unicode-ident|1.0.27|MIT + Unicode-3.0|[声明](licenses/unicode-ident-1.0.27)|


桌面窗口的音频设备使用 SDL2：Rust 绑定采用 MIT，所带 SDL 源码采用 Zlib，HIDAPI 选择 BSD-3-Clause，像素转换组件同为 BSD-3-Clause；这些许可与 LGPL-2.1 兼容。分发示例时随附以下声明，保留版权、不得冒用来源或作者背书，修改第三方源码时标明修改。当前直接构建未修改的上游源码，未启用 SDL 扩展库。

|包|版本|采用的许可|声明|
|:--|:--|:--|:--|
|sdl2|0.38.0|MIT|[许可](licenses/sdl2-0.38.0/LICENSE)|
|sdl2-sys|0.38.0|MIT + Zlib + BSD-3-Clause|[绑定](licenses/sdl2-sys-0.38.0/LICENSE-MIT)、[SDL](licenses/sdl2-sys-0.38.0/SDL/LICENSE.txt)、[HIDAPI](licenses/sdl2-sys-0.38.0/SDL/src/hidapi/LICENSE-bsd.txt)、[像素转换](licenses/sdl2-sys-0.38.0/SDL/src/video/yuv2rgb/LICENSE)|
|cmake|0.1.58|MIT|[许可](licenses/cmake-0.1.58/LICENSE-MIT)|
|lazy_static|1.5.1|MIT|[许可](licenses/lazy_static-1.5.1/LICENSE-MIT)|
|version-compare|0.1.1|MIT|[许可](licenses/version-compare-0.1.1/LICENSE)|
|cc|1.11.1|MIT|[许可](licenses/cc-1.11.1/LICENSE-MIT)|
|find-msvc-tools|0.1.15|MIT|[许可](licenses/find-msvc-tools-0.1.15/LICENSE-MIT)|
|shlex|2.0.1|MIT|[许可](licenses/shlex-2.0.1/LICENSE-MIT)|

SDL2 使用 bundled／static-link 构建，设备由同一套跨平台接口选择。CMake 4 构建旧版 SDL 时，需要最低策略版本 3.5；Cargo 配置提供可覆盖的默认环境值。
