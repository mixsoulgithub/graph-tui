[English](usage.md) | 中文

# 用法:命令行、出图、布局

## 打开文档

```bash
graph-tui metro.gtui                     # 打开原生文档(直接按文件里存的坐标画)
graph-tui examples/pipeline.dot          # 导入 .dot;第一次按 `w` 会在旁边写出 pipeline.gtui
graph-tui                                # 不给文件:空白文档(只在内存里,退出即弃)
graph-tui --dump --size 120x36 x.gtui    # 打一帧就退出,方便截图和写文档
```

不给文件、stdin 又是终端时,得到一份**空白文档**:内存里的一页纸(一个分组、一个图层、一个框
`a`)。不写任何临时文件,退出就没了。存盘必须自己给路径(`:w name.gtui`),不给就只回显
"还没有文件名"。stdin 是管道时仍然从管道读。

```
--file <path>      读文档(`-` 表示 stdin;直接把路径当参数给也一样)
--layout <name>    geo | relax | force | layered;默认按图自己挑
--dump             打一帧真彩 ANSI 到 stdout 后退出
                   输入按扩展名认格式:`.gtui` 原生、`.canvas` JSON Canvas、
                   其余都当 .dot
--size <WxH|auto>  --dump / --export 的画布尺寸(默认 auto:内容 + 留白;
                   小了就裁,不缩放)
--labels           开局就把点节点的名字都标出来(等于按 b)
--no-labels        不画点节点的名字(--dump / --export)
--format <ansi|text>  --dump 的输出:真彩 ANSI(默认)或纯文本
--legend           强制打开右侧图例(不给就听文档的)
--no-legend        强制关掉(和 --legend 互斥)
--no-notes         不画边注释(默认跟文档的 view.notes)
--drawn-cursor     光标画在画面里(默认用终端真光标;只影响 TUI,--dump/--export 不画光标)
--history <N>      撤销步数上限(默认 200)
--arrow <style>    箭头字形 ascii|arrows|triangles(默认 ascii)
--at <name>        开局把光标停在这个节点上
--find <text>      开局先搜一遍(名字 / 别名 / 分组号)
--lang <zh|en>     界面语言(默认 zh;命令行帮助与报错固定英文)
--export <path>    写出去就退出:.gtui(原生存档)/ .svg / .png(rsvg-convert 栅格化)/
                   .canvas(JSON Canvas)/ .dot(best-effort 有损视图,警告打到 stderr)
-v, --version      版本
-h, --help         帮助
```

### 出图:`--dump` / `--export`

只画图,不画界面:没有状态行、没有光标和选区。图例是画面的一部分,默认跟着文档走。

- **1:1**,永远不缩放;图居中,四周留白(左右各 2 列、上下各 1 行)。
- **自动尺寸**:不给 `--size` 时,先按 1:1 画一帧探针,量出真正有墨迹的矩形(框、文字、边、
  T 形接点、箭头、点节点标签),再把那块连留白一起抄出来。不会重画,所以自动放好的标签在探针
  与输出之间不会移动。
- `--size WxH` 是精确尺寸,仍然 1:1、仍然居中;**放不下就裁**,绝不缩小,并在 stderr 说明。
  `--size auto` 等于不给尺寸。
- `--format text` 打纯文本(没有转义序列);`--format ansi`(默认)是真彩。stdout 里只有画面,
  末尾只有一个换行。
- `--no-labels` 不画点节点名;dump 默认画。
- `--export x.svg` / `x.png` 用同一套自动尺寸;PNG 仍然先出 SVG 再走 `rsvg-convert`。
- 图例是画面的一部分:跟着 `view.legend`,画在地图右侧(至少隔 2 列,垂直居中);`--no-legend`
  去掉它。宽度上限 `min(终端宽度 / 3, 44)`,超长的行用 `...` 截断。
- 边注释也是内容:跟着 `view.notes`,`--no-notes` 去掉它们(藏起来的注释不算障碍、不占地方)。
- headless 的 `render` 操作一样:`size` 可以省略或给 `"auto"`;`"legend": false` 关掉图例,
  JSON 响应里带 `legend.rect`。

```bash
graph-tui --dump graph.gtui > graph.ansi
graph-tui --dump --format text graph.gtui > graph.txt
graph-tui --export graph.svg graph.gtui
```

## 布局

| 名字 | 做什么 | 要求 |
| --- | --- | --- |
| `geo` | 保留当前位置(只把重叠的框分开) | 任何文档 |
| `relax` | 从当前位置做弹簧松弛;匀开间距,把方向吸到 `- \| / \` | 任何文档 |
| `force` | Fruchterman-Reingold;结束时吸到水平/垂直/45 度 | 任何文档 |
| `layered` | Sugiyama 式分层;最适合有向的依赖/流程图 | 任何文档(含环) |

朝向(`view.orientation`):为 `auto` 时,如果转 90 度后需要的缩放更小,`force` / `layered`
就把图转过来(差距不到 5% 就不转,免得在阈值上抖);`geo` 与 `relax` 保留输入朝向,因为坐标
本身有意义。打开 `.gtui` 不会重算坐标,只是记住这个值。显式 `:rotate` 会改坐标(要求 1:1),
但不会交换框的宽高。

四套布局都产出**整数格**坐标,写回 `box.at`,并把附着的文字按同样位移搬走;任意两个框至少隔
一格。`r` 原地轮换这四套。**换布局永远不移动光标**;搜索与聚焦跟着框走。

四套布局都是**确定性的**:同一份文档跑两遍得到完全相同的点(没有随机)。

## 从零画图

不必先有 `.dot`:用空白文档启动 `graph-tui`,所有东西都用 `:` 动词造。

```
graph-tui                 # 空白文档,开局一个节点 a(默认样式 none:一行文字)
:new B                    # 在光标格新建节点,文字 B,光标跟过去
:box single               # 显式给它加个框
:link a                   # 从光标节点(B)连一条边到 a;目标可以是 id(b1)或文字
:box double               # 把光标框换成双线边框
:fill #1b2440             # 内部填充
:textcolor #f59e0b        # 文字颜色
:text hello world         # 替换文字(整行原样收下;**粗体** /斜体/ 生效;框会自己长)
Ctrl-e                    # 选中一条边;然后 :dir both / :corner sharp / :style dashed
:w name.gtui              # 存盘;没有路径就不写(:w 只回显一句提示)
:w ~/figs/name.gtui       # 开头的 ~ 展开成家目录(命令行参数也一样)
:q                        # 退出(只有 :q 退出);没存的内容就没了,不留临时文件
:wq                       # 存成 .gtui 并退出;失败就留在原地
```

`del` 删光标框(连文字与边);选中边时删那条边。最后一个框删不掉。新节点默认 `none`(一行
文字,宽度跟着文字走);`:box` 才加框。

`Ctrl-e` 选边,`:` 动词改属性,`:w` 存盘。`e` 是两段式连边:先定源(选区或光标框),再选目标,
`Enter` 建边、`Esc` 取消。1->1 就是"在 A 上按 `e`,走到 B 按 `Enter`";源和目标落在同一个框上
就建自环(节点一侧出、相邻一侧回的小钩)。1->N 就是"按 `e`,用 `v` 框选多个目标,`Enter`";
N->M 则先框选源。

## 编辑

**编辑只在 `1:1` 下可用**:缩放时只能看(`VIEW x2.0`)。输入编辑命令时会先把视图切回 `1:1`
并居中,再执行;底行显示 `VIEW`(缩放时 `VIEW x2`)。

在底行提示(新建节点 / 改文字)里按 **`Ctrl+G`**,把当前内容交给 `$VISUAL` / `$EDITOR`(都没有
就用 `vi`)。编辑器退出后读回文件替换输入行:内容没变(`:q!`、立刻退出、不阻塞的 GUI 编辑器)
算取消;退出码非 0 时保留原文并报出退出码;非法 UTF-8、CR、NUL 一律拒绝。仍然要按 `Enter`
提交,`Esc` 丢弃。编辑器必须**阻塞**(`code -w` / `vim`);命令行和搜索不接受这个键(按了只
提示)。

选中一条边时,`i` 新建或编辑它的注释框(空框用 `Esc` 或空回车撤掉);`a` 或 `:notes` 收起/展开
所有注释;`:relabel [all]` 把注释框重新锚回它们的边。注释收起时不画、不算障碍、不占空间。

有选区时,`box` / `nodecolor` / `fill` / `textcolor` / `textalign` / `text` 作用于**整个选区**;
`L` 只显示一个图层时,动词只动那一层。所有动词见 [docs/keybindings.zh.md](keybindings.zh.md);
样式总览是 `examples/styles-all.gtui`(六种框型、粗体/斜体、文字色、填充、三种对齐、三种线型、
四种方向、两种拐角同在一页),导入示例在 `examples/styles.dot`;边注释见
`examples/edge-notes.gtui`(五条边,各带一个注释框);多行文字见 `examples/multiline.gtui`
(一个三行带框的框、一个两行的纯文字节点、一个两行的边注释)。

撤销/重做是快照式的,默认 200 步(`--history N`);设置同一个值不消耗一步,整段拖拽算一步,
只改视图的动作不进栈。详见 [docs/keybindings.zh.md](keybindings.zh.md)。

图层决定重叠元素的上下顺序:框和边都带 `layer`,按图层顺序绘制、当前图层在最上面,当前图层
之外的元素压暗。移动框会同时改 `box.at` 和它的 `text.at`(整数);坐标一旦存盘就固定,只有
`r` 会重新布局。
