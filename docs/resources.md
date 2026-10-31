# 资源名称与类别

本文固定使用 Windows CatSystem2 2.6.1.90 的名称补全规则。脚本中的资源名先按配置类别解析为虚拟名称，再由资源宿主查找归档或散装文件。

## 配置与接口

启动配置的 `INT/intNN` 条目使用 `name`、`int`、`ext`、`dir` 描述类别标识、归档路径、缺省扩展名和散装目录。`resource::Category` 接收 `int` 对应的虚拟名前缀及 `ext` 对应的扩展名，均保留原始字节；`resolve(input)` 返回补全后的虚拟名称。

```rust
use katze_system2::resource::Category;

let category = Category {
    prefix: b"mot".to_vec(),
    extension: b"kcs".to_vec(),
};
assert_eq!(category.resolve(b"sample").unwrap(), b"mot/sample.kcs");
```

`ArchiveConfig::from_xml` 读取已解码为 UTF-8 的启动 XML，`find(name)` 返回类别槽位，`resolve(category, name)` 完成类别查找及名称补全。归档装载、补丁覆盖及散装目录查找由资源宿主负责。`Category` 只处理名称；不检查文件是否存在，不访问文件系统，也不规范化目录或大小写。

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

## 启动配置

`ArchiveConfig` 提供 128 个 `entries` 和 64 项 `required`。每个 `ArchiveEntry` 包含类别标识 `name`、名称补全参数 `category` 以及散装目录 `directory`。默认条目的四个字符串均为空，必需归档列表默认为 −1。

```rust
use katze_system2::resource::ArchiveConfig;

let config = ArchiveConfig::from_xml(r#"<?xml version="1.0" encoding="UTF-8"?>
<document><INT><int00>
  <name>mot</name><int>motion.int</int><ext>kcs</ext><dir>loose</dir>
</int00><need00>0</need00></INT></document>"#)?;
assert_eq!(config.find(b"MOT"), Some(0));
assert_eq!(config.resolve(b"mot", b"sample")?, b"motion.int/sample.kcs");
# Ok::<(), katze_system2::Error>(())
```

`name` 是类别查找标识，`int` 是加入虚拟名称的归档前缀，两者具有不同用途；`dir` 不参与虚拟名称补全。实际配置的 50 个条目中，两字段全部不同，`int` 全部为带归档扩展名的路径。虚拟名称的归档部分和归档内部条目名需由资源宿主分开处理。

XML 路径及字段名称区分大小写，类别标识查询忽略 ASCII 大小写，按槽位顺序选择首项。空标识会匹配第一个空槽位。`int00`～`int127` 采用十进制编号，少于两位时补零；`int0` 和超出容量的编号不参与读取。重复 `INT`、重复槽位及字段按路径合并，后出现的同一字段覆盖旧值，未重写的其他字段保留。

`need00`～`need63` 存放必需归档的槽位编号，各项保留原始 i32；它们不是各个类别的布尔开关。缺失项为 −1，已出现的空文本或无数字前缀为零。数值支持正负号、十进制和 `0x` 十六进制，32 位回绕，忽略数字后的普通后缀；十进制数字后紧接点时返回零。前导空白不会跳过。

基线读取器要求 XML 声明位于首字节，版本为 `1.0`，编码为 `UTF-8`；前置 BOM、空白、CDATA 和嵌套自闭合节点返回诊断。空字段使用成对标签。字符串不去除首尾空格，CRLF／LF 和 Tab 转为空格，单独 CR 保留；注释和子元素内容从字段的直接文本中排除。实体引用保留字面文本，例如 `a&amp;b` 读取为同样的七个字节，不展开成 `a&b`。非 ASCII 字段保留 UTF-8 字节。

库按原固定字段容量限制读取：`name` 和 `int` 各最多 31 字节，`ext` 最多 7 字节，`dir` 最多 259 字节，超长返回诊断。带目录的资源名在类别查找之前直接返回，因此此路径不要求类别存在；仅开头分隔符仍执行类别查找。

34 份合成 XML 的装载结果、13,501 个字段与查询，以及一份实际启动配置的全部 128 个槽位、64 项必需列表和类别查询，均已对照原入口。实际配置的 50 个类别和 24 个非负必需编号全部一致。超出原固定字段容量的合成输入由库返回诊断，避免原路径的缓冲区覆盖。
