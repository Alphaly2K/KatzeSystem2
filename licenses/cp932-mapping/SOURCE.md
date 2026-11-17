# CP932 转换映射

补充映射按 Unicode 公布的 Microsoft Windows CP932 数据提取：

- [数据](https://www.unicode.org/Public/MAPPINGS/VENDORS/MICSFT/WindowsBestFit/bestfit932.txt)
- [格式说明](https://www.unicode.org/Public/MAPPINGS/VENDORS/MICSFT/WindowsBestFit/readme.txt)
- [许可适用范围](https://www.unicode.org/copyright.html)

源文件 SHA-256：`2614cfea35c3c86c41d33198793a84ca44edee3cf0ee0013a61a43fba4ece331`。

`src/text/cp932.rs` 保留与现有严格编码路径不同的 84 项 BMP 映射；另以连续区间表示 U+E000～U+E757 的 1,880 个用户定义字符。公布数据中未登记的单元使用默认字节 0x3F。原数据与派生映射适用 [Unicode License v3](LICENSE-UNICODE)；分发时保留版权和许可声明，不得使用版权人名称为产品背书。
