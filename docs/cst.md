# CST 剧情脚本

适用基线：CatSystem2 2.6.1.90。数值词法见[表达式](expressions.md)，本地化数据见 [CSTL](cstl.md)。

## 运行时上下文

普通 323／324 已通过 Runtime 接入独立 CST 上下文的创建和释放。创建时从全局语言配置复制前缀，按原始字节排序去重，空名称同样注册；修改全局语言配置只影响后续创建。初始没有装载脚本，`Runtime::cst_context`、`CstContext::languages` 和 `script` 提供只读查询。句柄与图像及其他数据对象共用递增编号源，不复用，创建任务结束后继续存活，须显式释放。325 资源装载、本地化表绑定及普通脚本驱动的事件分派仍待接入；下文为已有脚本解析和解释器契约。

96 份原构造快照核实语言注册、重复名、空名、大小写、长名称及配置变化后的实例独立性，Rust 结果一致。常规合成回归连接普通任务、两种预算、共享句柄及显式释放，不依赖设备或外部材料。

## 源文本与运行时记录

官方 `mc.exe` 将剧情源文本编译为 CST。源文件中的 `#label` 决定产生的场景文件；一个源文件可以产生多个 CST。源宏和包含文件属于编译阶段。

源文本的主要规则为：

- 官方命令按手册使用半角小写字母；扩展名称保留原大小写。官方手册规定资源名和场景标签名不区分大小写。
- 姓名和正文由行首及半角空白区分。源文本中的空行会影响消息块划分，不能先全局删除。
- 支持 `//` 行注释，不支持 `/* ... */` 块注释。
- `#include`、`%` 宏定义和宏参数按源编译语法处理。已编译 CST 中的宏调用已展开。

从归档读取 CST 时，直接使用其类型化记录。不要根据缩进重新推断姓名、正文和命令。

## 二进制结构

所有整数为小端。文件头共 16 字节。

|偏移|类型|字段|
|:--|:--|:--|
|`0x00`|8 字节|签名 `CatScene`|
|`0x08`|u32|压缩长度 C|
|`0x0C`|u32|解压长度 U|
|`0x10`|字节序列|脚本主体|

`C>0` 时主体为 Zlib 数据；公开格式也定义了 `C=0` 时直接存储 U 字节的写法。本次 604 份 CST 均为压缩形式。

解压主体的前 16 字节为：

|偏移|类型|字段|
|:--|:--|:--|
|`0x00`|u32|主体长度，不含这 16 字节|
|`0x04`|u32|输入区间数 N|
|`0x08`|u32|记录偏移表相对位置 O|
|`0x0C`|u32|字符串区相对位置 S|

O 和 S 相对于主体头结束处。记录数为 `(S-O)/4`；每个记录偏移相对于字符串区起点。

```text
主体头
输入区间表：N × 8 字节
记录偏移表：每项 u32
字符串区：01 type payload 00
```

基线核实到输入区间的每项为 `(区间记录数, 起始记录索引)`，连续区间覆盖整个记录序列。除最后区间外，各区间末尾为输入等待记录；最后区间包含末次输入后的命令。本次所有文件均满足 `N = 输入等待记录数 + 1`，最后区间均非空。

这些是记录索引和记录数量，不是字节地址，也不是本地化消息表的条目数。

## 记录类型

|值|含义|处理边界|
|:--|:--|:--|
|`0x02`|Input|当前消息之后等待输入|
|`0x03`|Page|翻页并等待输入；本次样本未使用|
|`0x20`|Message|向消息系统提供正文与内嵌标记|
|`0x21`|Name|设置发言人|
|`0x30`|Command|解析命令文本|
|`0xF0`|ScriptName|源文件调试信息；本次样本未使用|
|`0xF1`|LineNumber|源行号调试信息；本次样本未使用|

正文和姓名在基础 CST 中为 CP932。CSTL 替换文本具有独立的 UTF-8 编码；配置中的场景编码值不能用于重新解释所有原始 CST 字节。

## 命令语法

```ebnf
command       = assignment
              | "if", "(", expression, ")", singleCommand
              | imageCommand
              | namedCommand
              | choiceRow ;
imageCommand  = imageType, [ selector, [ argumentTail ] ] ;
imageType     = "bg" | "cg" | "fg" | "eg" | "fw" | "pl" ;
selector      = integer | "[", integer, "-", integer, "]"
              | "[", integer, { ",", integer }, "]" ;
namedCommand  = commandName, argumentTail ;
choiceRow     = integer, target, choiceText ;
```

这里省略了用于区分参数的空白符。`argumentTail` 的结构由具体命令决定；选择器的区间和枚举支持范围也由命令决定。

银行号读取完整整数，不能限制为一位数字。区间和枚举是两种写法，手册不接受 `[0,2-4]` 这样的混合形式。只有选择项收集上下文中的数字开头记录才按 `choiceRow` 解释。

部分图像命令省略银行或资源参数时具有初始化／清理用途，按具体类型的签名处理。

### 条件

`if (expr) command` 只控制同一记录中的一个命令。条件非零时执行，否则继续下一记录。CST 没有由该语法派生的 `else/endif` 块。

手册示例包含右括号后直接连接命令的写法；本次 38,739 条条件均使用空白分隔。条件边界应由匹配的右括号确定。

```text
#8=3
if (#8>=2) next demo_route
next demo_fallback
```

### 场景与调用

|写法|手册规则|
|:--|:--|
|`next target`|加载目标场景并转移执行|
|`call target`|保存返回位置并调用目标场景；支持嵌套|
|`return`|提前返回调用者|
|被调用场景到达末尾|返回调用者，无需显式 `next`|

样本有大量 `call` 和 `next`，没有固定的 `start.cst`。场景初始入口应来自启动／宿主流程。

### 选择项

```text
fselect demo_choices
1 demo_left Left
2 demo_right Right
```

`fselect [界面脚本]` 后面跟随 ID、目标和显示文本。手册规定 ID 可以重复，因此选择列表需要保留顺序和每个条目，不能按 ID 去重。

原收集函数逐条读取后续 Command，并处理可选的 `if`。条件为假时跳过该记录；条件为真时提取 ID、目标和文本，向结果列表追加一项。遇到其他记录类型或数据末尾时结束；若当前 Command 缺少必需字段，则退回该记录，交回外层解释流程。保留下来的选项使用从零开始的连续本地化序号，详见 [CSTL](cstl.md)。选项窗口的调用、交互和返回调度仍待核实。

### 字符串

|写法|含义|
|:--|:--|
|`str n value`|写入字符串变量|
|`strn n value`|将数值转成十进制字符串|
|`$strN`|在正文或参数中引用字符串变量|
|`strcmp dst a b [caseSensitive]`|比较后写入数值变量；相同为 0|

字符串引用可嵌入资源名，不限于独立参数。原读取函数在读取记录时展开 `$str`；Command 随后将剩余 `$` 替换为 `0x`，Name 和 Message 走文本分支，见[表达式](expressions.md)。手册对 `strn` 的紧邻正文引用还有延迟限制，其与消息缓存的关系仍待核实。

## 等待与调度

手册的时间单位为 **1 帧 = 1/60 秒**。

|写法|手册规则|
|:--|:--|
|`wait`|等待之前的相关运动、效果完成|
|`wait frames`|等待给定时间；frames 是数值表达式|
|`wait key`|等待输入|
|`wait pcm [bank]`|等待语音结束|
|`wait se [bank]`|等待音效结束|
|`wait pt [bank]`|等待粒子结束|
|`wait tag name [fallbackFrames]`|等待当前音频中的标签，未找到时按备用时长等待|

无参等待不自动等待 BGM 或 SE 的完整播放。循环动作、被停止的动作、快进、音频尚未加载及并发动作的等待状态需要有明确结束条件。

基线还使用 `wait mpl ...`，其等待对象及完成条件尚待确认。样本也使用算术表达式作为时长，不能仅接受连续十进制数字。

手册将部分画面命令的提交边界放在消息、输入或等待处，音频加载则可以立即开始播放。`rdraw` 表示画面重绘策略，其默认时间由 `drawdef` 设置；不能统一简化为“每张图各自淡入”或“所有命令都立即提交”。

## 文本标记

正文的语法与命令语法分开解析，至少保留文本片段及其控制标记。

|写法|手册规则|
|:--|:--|
|`\n`|正文内换行；源文本中独立成行还有输入等待含义|
|`\@`|使后续消息／命令延续到之后的等待或输入边界|
|`[word/ruby]`|注音|
|`[word]`|禁止词内断行|
|`\\`、`\[`|显示字面反斜杠、左方括号|
|`\c0x...;`、`\cr0x...;`、`\cs0x...;`|正文、注音、阴影颜色，顺序为 AABBGGRR|
|`\f...`、`\fr...`|字体及注音尺寸|
|`\pl`、`\pc`、`\pr`|对齐方式|
|`\w...;`、`\wf...;`|字符显示等待或按帧等待|

标记的大小写不敏感。字体和颜色标记有默认恢复写法和消息页范围，具体绘制由消息系统处理。已接入的基线解析与具体恢复消费规则见 [消息标记接口](message.md)。将方括号直接删除、把 `\n` 换成空格或每遇 Message 就覆盖全部正文，都会丢失语义。

## 基线覆盖与未分类命令

本次 604 份 CST 共 401,255 条记录，使用了 Command、Name、Message 和 Input。出现的命令家族包括画面／运动、音频、条件、字符串、调用、选择、FES 交互、粒子和相机等。

其中 `mpl`、`cam` 及部分扩展调用超出已查阅手册的说明范围；还存在未分类或疑似拼写异常的命令头。命令识别应保存原始记录及位置，并将“可解析语法”和“已实现语义”分别统计。未知命令的原引擎容错和可达性待核实，不依据拼写自动修正或静默跳过。

## 资料

- [CST 文件结构](https://github.com/trigger-segfault/TriggersTools.CatSystem2/wiki/CST-Scene)
- [源文本规则](https://cs2.suki.jp/manual/data/text/text.html)
- [对象、银行和时间单位](https://cs2.suki.jp/manual/scene/begin/1_use.html)
- [条件](https://cs2.suki.jp/manual/scene/reference/unique/if.html)、[场景跳转](https://cs2.suki.jp/manual/scene/reference/unique/next.html)、[子程序](https://cs2.suki.jp/manual/scene/reference/unique/subroutine.html)
- [等待](https://cs2.suki.jp/manual/scene/reference/unique/wait.html)、[命令提交时机](https://cs2.suki.jp/manual/scene/general/2_wait.html)、[画面重绘](https://cs2.suki.jp/manual/scene/general/3_draw.html)
- [文本标记](https://cs2.suki.jp/manual/scene/reference/escape.html)、[选择项](https://cs2.suki.jp/manual/scene/reference/unique/fselect.html)、[命令目录](https://cs2.suki.jp/manual/scene/reference/contents.html)
