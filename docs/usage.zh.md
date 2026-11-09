[English](usage.md) | 中文

# 用法:命令行、出图、布局

## 打开文档

```bash
graph-tui metro.gtui                     # 打开原生文档(直接按文件里存的坐标画)
graph-tui new.gtui                       # 文件还不存在:新文件,`:w` 才写盘(像 nvim)
graph-tui examples/pipeline.dot          # 导入 .dot;第一次按 `w` 会在旁边写出 pipeline.gtui
graph-tui                                # 不给文件:空白文档(只在内存里,退出即弃)
graph-tui --dump --size 120x36 x.gtui    # 打一帧就退出,方便截图和写文档
graph-tui tutor                          # 内置教程:一份 scratch 副本(目前为英文)
```

`graph-tui tutor` 打开编进二进制的教程文档(8 个短章节加一个 sandbox,目前只有英文)。
它是一份普通文档,放在 scratch 缓冲里:可以随便改、改坏、用 `r` 换布局,不会写回任何文件;
`:q` 退出,重新运行命令就从原文开始。它不接受文件参数,也不接受 `--dump` / `--export`。

如果终端画不了制表符(或会把它们画成两格宽),`--ascii` 会用 `- | + o @` 画全部东西。
拐角、T 形接点与交叉都变成 `+`,双线/粗线框退回单线;颜色与几何不变。`[prefs] render = ascii`
可以让它成为默认。

多个文件会开成标签页(`graph-tui a.gtui b.dot`);每个标签页有自己的文档、光标、缩放、撤销
历史和文件戳,剪贴板共享。`Ctrl-t` / `Ctrl-n` / `Ctrl-p` / `Ctrl-w`(或 `:tabnew` /
`:tabnext` / `:tabprev` / `:tabclose`)管理它们,状态行显示 `[2/4] name *`。`:tabclose`
遇到未保存改动会拒绝,除非加 `!`;`:e` 也不会替换有改动的文档。

打开文件总是从 `1:1` 开始,以画面为中心(等同按 `0`);文件里存的 `zoom` / `center` 只给
`--dump` 与导出用,交互式打开不恢复它们。要看全就按 `F`。

不给文件、stdin 又是终端时,得到一份**空白文档**:内存里的一页纸(一个分组、一个图层、一个框
`a`)。不写任何临时文件,退出就没了。存盘必须自己给路径(`:w name.gtui`),不给就只回显
"还没有文件名"。stdin 是管道时仍然从管道读。

一个还不存在的 `.gtui` 路径会像 `nvim newfile` 那样打开:已经绑定这个文件名的空文档
(状态行写着 `new file … (:w to write it)`),`:w` 就创建文件。`.dot` / `.canvas` 只作导入,
所以缺文件仍然是错误,`--dump` / `--export` 也需要真实内容。同一条规则适用于 `:e new.gtui`
和 `:tabnew new.gtui`;在这种缓冲上按 `R` 只会提示文件还不存在,而不是重新载入一个空文档。

文件打开期间会盯着磁盘:别的程序改写它时,没有本地改动的文档会自动重新载入,有改动的会得到
一条持续警告,并给两条路(`R` 重载并丢弃改动,`:w` 覆盖文件)。见
[keybindings](keybindings.zh.md)。

按 Enter **之前**就能看到 `:` 动词会读/写哪里:状态行右侧会解析路径。对 `.gtui` 按 `:w`
显示文件加 `(overwrite)`;对导入的 `.dot` 显示将要创建的 `<stem>.gtui`,并加
`(source .dot stays untouched)`。`:w` / `:wq` 之后按 `Tab` 会把默认路径填进输入行,仍可编辑;
`:e` / `:w` / `:export` 之后按 `Tab` 补真实路径(目录优先,展开 `~/`,保留带空格的名字)。

```
--file <path>      读文档(`-` 表示 stdin;直接把路径当参数给也一样)
--layout <name>    auto | geo | relax | force | layered | tree | chain;默认 auto
                   (按图的结构挑)
--dump             打一帧真彩 ANSI 到 stdout 后退出
                   输入按扩展名认格式:`.gtui` 原生、`.canvas` JSON Canvas、
                   其余都当 .dot
--size <WxH|auto>  --dump / --export 的画布尺寸(默认 auto:内容 + 留白;
                   小了就裁,不缩放)
--labels           开局就把点节点的名字都标出来(等于按 `B`)
--no-labels        不画点节点的名字(--dump / --export)
--format <ansi|text>  --dump 的输出:真彩 ANSI(默认)或纯文本
--no-notes         不画边注释(默认跟文档的 view.notes)
--drawn-cursor     光标画在画面里(默认用终端真光标;只影响 TUI,--dump/--export 不画光标)
--history <N>      撤销步数上限(默认 200)
--ascii            纯 ASCII 字形(框线、接点、点节点),给画不了制表符的终端用;
                   --unicode(默认)恢复制表符
--lane-gap <N>     分层布局:同层相邻两框的最小列距(默认 3)
--layer-gap <N>    分层布局:层与层之间的最小行距(默认 3)
--layout-margin <N>  布局后的画面四周留 N 个空格(默认 0)
--arrow <style>    箭头字形 ascii|arrows|triangles(默认 ascii)
--at <name>        开局把光标停在这个节点上
--find <text>      开局先搜一遍(名字 / 别名 / 分组号)
--lang <en|zh>     界面语言(默认 zh;命令行帮助与报错始终英文)
--config <path>    读这个配置文件(默认:$GRAPH_TUI_CONFIG 或
                   $XDG_CONFIG_HOME/graph-tui/config.ini;显式给的路径不存在就报错)
--no-config        忽略所有配置文件
--print-config     打印完整的默认配置(prefs + 所有键)后退出
--init-config [p]  把默认配置写到 [p] 或默认路径;--force 才覆盖
--force            允许 --init-config 覆盖已有文件
--export <path>    写出去就退出:.gtui(原生存档)/ .svg / .png(rsvg-convert 栅格化)/
                   .canvas(JSON Canvas)/ .dot(best-effort 有损视图,警告打到 stderr)
-v, --version      版本
-h, --help         帮助
```

### 出图:`--dump` / `--export`

只画图,不画界面:没有状态行、没有光标和选区。

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
  一格是 9x18 px,字体 15 px,所以文字步进与格子宽度一致。边框与线字形
  (`─ │ ┌ ┐ └ ┘ ├ ┤ ┬ ┴ ┼ ╭ ╮ ╰ ╯`,粗线版本也一样)输出成格边之间的 `<path>` 段,而不是
  字体字符,所以无论看图的人选什么字体,笔画都连在一起;其余字形在格子里居中
  (`text-anchor="middle"`),所以步进不是正好 9 px 的字体也不会在一行里漂。
- 边注释也是内容:跟着 `view.notes`,`--no-notes` 去掉它们(藏起来的注释不算障碍、不占地方)。
- 没有图例面板了。画面需要键时,把它画成内容:一个带文字的 `none` 或带框的框、一条用来展示
  线型的短边,以及你已有的分组颜色。用 `geo` 布局让它原地不动(或者在布局跑完之后再摆),
  因为 `auto` / `layered` 会把看到的每个框都挪走。
- headless 的 `render` 操作一样:`size` 可以省略或给 `"auto"`,响应里带 `size`、`lines`、
  `labels_skipped` 与 `warnings`。

```bash
graph-tui --dump graph.gtui > graph.ansi
graph-tui --dump --format text graph.gtui > graph.txt
graph-tui --export graph.svg graph.gtui
```

## 布局

| 名字 | 做什么 | 要求 |
| --- | --- | --- |
| `auto` | **默认**:读结构,从下面挑一套(chain / tree / layered / relax),把不连通的量打包,对团状图退回 `relax` 并给 `dense_graph` 警告 | 任何文档 |
| `tree` | 整齐树:父在上、居中于子树,兄弟保持原顺序;多余边照常走线,多父树用第一个父。纯星形(一个中心、其余节点度数都是 1)改画成**扇**:叶子最多排四列,每列十个,左右各两列,中心在竖直中线上;40 个叶子变成 `61x19` 的块,而不是 `527` 格宽的一行(解析出的 profile 是 `star`) | 任何文档(不是树时是近似) |
| `chain` | 把最长路径拉成直线,分支挂在下面;纯环确定性地断掉一条边 | 任何文档(否则是近似) |
| `geo` | 保留当前位置(只把重叠的框分开) | 任何文档 |
| `relax` | 从当前位置做弹簧松弛;匀开间距,把方向吸到 `- \| / \`。间距按轴量(框只有一行高,所以叠放的框是隔行,不是隔一个框宽),另有一个锚点把结果拉在 `sqrt(n)` 格见方附近,不让它一直往外爬 | 任何文档 |
| `force` | Fruchterman-Reingold;结束时吸到水平/垂直/45 度 | 任何文档 |
| `layered` | Sugiyama 式分层:先断回边,长边插入哑节点让中间层能绕开它们排序,然后用交替重心扫描加一次转置选交叉最少的顺序;最适合有向的依赖/流程图 | 任何文档(含环) |

新文档与 `.dot` / `.canvas` 导入用的就是 `auto`(`view.layout` 存的是这个名字,解析出的
profile 不会写回)。`r` 在六套具体布局间循环 `geo -> relax -> force -> layered -> tree ->
chain -> geo`;`auto` 只能用 `:layout auto` 或 `--layout auto` 进入。状态行报解析出的 profile
(`layout = auto(tree)`、`layout = auto(star)`),headless 的 `layout` 操作返回 `profile`、
`components`、`crossings`、`reversed` 与 `warnings`(`dense_graph`、`components_packed`、
`layout_tree_approx`、`layout_chain_approx`、`edges_reversed`)。扇只对作为辐条**源**的中心
生效(或对无向辐条);当箭头全部指向同一个节点时,端口会堆在一侧,所以那种情况仍走普通树。

朝向(`view.orientation`):为 `auto` 时,如果转 90 度后需要的缩放更小,`force` / `layered`
就把图转过来(差距不到 5% 就不转,免得在阈值上抖);`geo` 与 `relax` 保留输入朝向,因为坐标
本身有意义。打开 `.gtui` 不会重算坐标,只是记住这个值。显式 `:rotate` 会改坐标(要求 1:1),
但不会交换框的宽高。

所有布局都产出**整数格**坐标,写回 `box.at`,并把附着的文字按同样位移搬走;任意两个框至少隔
一格。`r` 原地轮换六套具体布局。**换布局永远不移动光标**;搜索与聚焦跟着框走。

所有布局都是**确定性的**:同一份文档跑两遍得到完全相同的点(没有随机)。

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
:text hello world         # 替换文字(整行原样收下;**粗体** *斜体* 生效;框会自己长)
Ctrl-e                    # 选中一条边;然后 :dir both / :corner sharp / :head triangles / :style dashed
:w name.gtui              # 存盘;没有路径就不写(:w 只回显一句提示)
:w ~/figs/name.gtui       # 开头的 ~ 展开成家目录(命令行参数也一样)
:q                        # 退出(只有 :q 退出);没存的内容就没了,不留临时文件
:wq                       # 存成 .gtui 并退出;失败就留在原地
```

`del` 删光标框(连文字与边);选中边时删那条边。最后一个框删不掉。新节点默认 `none`(一行
文字,宽度跟着文字走);`:box` 才加框。新框会复制你最后选中过的框的样式、颜色与对齐;用
`s` 菜单 / `:box` / 颜色动词刚改过样式的框也算被选中,所以模板不会因为光标移走而丢。区块
选区不算。

改框的样式会把**文字**留在原地,让框跟着样式变:四种带框样式尺寸相同,它们之间切换什么都不
变;切到 `none` 就把框缩到文字本身,结果和这个节点一直是 `none` 的画面逐字节相同(边会重新
走到文字上,理应如此)。`point` 把节点字形放在第一个字符的格子上,名字放到旁边。反过来切,
框会围着文字长出来,不动文字。用输入而不是改样式时,`none` 节点仍像编辑器一样往右长。

`Ctrl-e` 选边,`:` 动词改属性,`:w` 存盘。`e` 是两段式连边:先定源(选区或光标框),再选目标,
`Enter` 建边、`Esc` 取消。1->1 就是"在 A 上按 `e`,走到 B 按 `Enter`";源和目标落在同一个框上
就建自环(节点一侧出、相邻一侧回的小钩)。1->N 就是"按 `e`,用 `v` 框选多个目标,`Enter`";
N->M 则先框选源。

## 编辑

**区域**(`:region`)把框收进一个带标题的框里。框的区域是成员包围盒加 padding,所以会跟着
成员走;`:region collapse` 用一条标题栏代替框并隐藏成员。`.dot` 的 `subgraph cluster_*`
导入时变成区域,导出时写回去(颜色、样式与收起状态有损)。区域画在边与框下面,所以永远不会
盖住文字。

**命令与输入要求 `1:1`**:缩放时只能看(`VIEW 150%`,细节降低时还会标 `LOD`)。输入编辑命令
时会先把视图切回 `1:1` 并锚在光标上,再执行;状态行显示 `VIEW`(缩放时为 `VIEW 150%`)。
鼠标拖框与 `:layout` / `r` 在任意缩放下都能用,而且你正在看的框会留在原处。

边端点可以钉住:选中一条边,用 `:fromside left` / `:toside up`(或 `:port from=left to=up`)
让它从指定侧面离开或到达。`auto` 是默认,按两个中心挑侧面,所以不主动改就什么都不变。点节点
没有边框,钉住的侧面在那里会被忽略。

连边也是鼠标手势:从带框节点的**边框**拖到另一个框上建边,拖选中边的**端点**到另一个框上
重新接上(颜色、分组、样式与注释保留)。在框内部拖仍然是移动它;靠近视口边缘时会顺手平移
视图。键盘 `e` / `:link` 仍是等价入口。

缩放有锚点:滚轮固定指针下的格子,`+` / `-` 固定光标格(每按一次 1.25,每个滚轮档 1.1,
离 `100%` 6% 以内会被吸附过去)。低于 `60%` 时文字完全不画,状态行标 `LOD`;低于 `30%`
时框变成实心块。`--dump` 与图片导出总是画全细节。

在底行提示(新建节点 / 改文字)里按 **`Ctrl+G`**,把当前内容交给 `$VISUAL` / `$EDITOR`
(都没有就用 `vi`)。编辑器退出后读回文件替换输入行:内容没变(`:q!`、立刻退出、不阻塞的
GUI 编辑器)算取消;退出码非 0 时保留原文并报出退出码;非法 UTF-8、CR、NUL 一律拒绝。仍然
要按 `Enter` 提交,`Esc` 丢弃。编辑器必须**阻塞**(`code -w` / `vim`);命令行和搜索不接受
这个键(按了只提示)。普通模式下 `Ctrl+G` 跳过提示行:把光标框(或整个选区)交给编辑器,
退出时结果直接写回,一步撤销。

选中一条边时,`i` 新建或编辑它的注释框(空框用 `Esc` 或空回车撤掉);`:notes` 收起/展开
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
