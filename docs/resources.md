# 资源名称与类别

本文固定使用 Windows CatSystem2 2.6.1.90 的名称补全规则。脚本中的资源名先按配置类别解析为虚拟名称，再由资源宿主查找归档或散装文件。

## 配置与接口

启动配置的 `INT/intNN` 条目使用 `name`、`int`、`ext`、`dir` 描述类别、归档、缺省扩展名和散装目录。`resource::Category` 接收 `name` 对应的虚拟名前缀及 `ext` 对应的扩展名，均使用原始 CP932 字节；`resolve(input)` 返回补全后的虚拟名称。

```rust
use katze_system2::resource::Category;

let category = Category {
    prefix: b"mot".to_vec(),
    extension: b"kcs".to_vec(),
};
assert_eq!(category.resolve(b"sample").unwrap(), b"mot/sample.kcs");
```

XML 配置读取、类别选择、归档装载、补丁覆盖及散装目录查找由资源宿主负责。`Category` 只处理名称；不检查文件是否存在，不访问文件系统，也不规范化目录或大小写。

## 已确认的补全规则

输入、前缀和扩展名都在第一个零字节处结束。

1. 按基线双字节范围扫描名称并记下最后一个目录分隔符。分隔符为 `/` 或 `\`。最后一个分隔符的下标大于零时，直接返回输入。
2. 其余名称检查最后一个路径分量中是否包含点。没有点时，依次追加一个点和配置扩展名；空扩展名仍追加点。
3. 前缀非空时，返回 `prefix/name`；空前缀直接返回名称。原有末尾斜杠、开头斜杠及点均保留。

|输入|前缀 `mot`、扩展名 `kcs` 时的结果|
|:--|:--|
|`sample`|`mot/sample.kcs`|
|`sample.KCS`|`mot/sample.KCS`|
|`sample.`|`mot/sample.`|
|`folder/sample`|`folder/sample`|
|`/sample`|`mot//sample.kcs`|
|空名称|`mot/.kcs`|

目录扫描的双字节首字节范围为 `0x81～0x9F` 或 `0xE0～0xEF`，尾字节为 `0x40～0x7E` 或 `0x80～0xFC`。合法双字节中的 `0x5C` 不作为目录分隔符。随后的扩展名检查按原始字节反向查找，使用另一套路径分量判断；包含此类字节的名称可能继续追加扩展名。两步按各自规则执行。

合成原入口对照覆盖 384 组前缀、扩展名、目录、点、空输入和双字节边界，结果全部一致。
