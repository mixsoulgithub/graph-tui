[English](examples.md) | 中文

# 内置示例

这些图故意把引擎往极限上推:

| 文件 | 压的是什么 |
| --- | --- |
| `star40.dot` | 一个中心接 40 个叶子:一格只有四个方向,扇出到了极限;`auto` / `tree` 会把它画成扇(四列、每列十个,中心在中线),`61x19`,而不是 `527x6` |
| `k12.dot` | K12 完全图:66 条边塞进 12 个点 |
| `grid20.dot` | 20x20 网格,400 点,760 条边:密度与耗时 |
| `chain60.dot` | 一条 60 节的链走 `layered`:3 格层高 x 60 层,窗口太矮就叠起来 |
| `ring.dot` | 一个有向环;`layered` 布局必须断掉一条回边 |
| `parallel.dot` | 同一对节点上跨四个分组有四条边:各自画成平行线(边框上没有格子可挪,所以靠车道偏移) |
| `selfloop.dot` | 自环:从一侧出,从相邻一侧回 |
| `isolated.dot` | 一个孤立节点加一个连通分量 |
| `components.dot` | 四个互不相连的分量 |
| `inarrow.dot` | 12 条边进同一个点:箭头只有四个方向 |
| `collinear.dot` | 八个共线节点 |
| `samepos.dot` | 五个坐标完全相同的节点 |
| `outlier.dot` | 一个远处的离群点把包围盒撑大 |
| `dupname.dot` | 两个同名节点(.dot 语义里算一个) |
| `longlabel.dot` / `cjk.dot` | 超长标签与中文标签宽度 |

它们量的是引擎的边角:投影之后节点可能落到同一格,线可能没地方走,标签可能放不下,但一切都
照画(重叠就让它重叠),状态行也不报这些细节;那一行留给命令和消息。

## 结构 profile(examples/)

这两个文件故意做得很小:它们用来钉住 `auto` 会挑什么,以及 `.dot` 导入怎么映射到区域和端口。

| 文件 | 展示什么 |
| --- | --- |
| `org.dot` | 一棵纯树,带两个 `subgraph cluster_*` 块(导入成区域)和一条带 `from_side` / `to_side` 的边;`auto` 解析成 `tree`,框跟着成员走,端口经 `.dot` 往返后仍在 |
| `timeline.dot` | 一条纯链;`auto` 解析成 `chain`,整条流水线落在同一行 |
| `limits/star40.dot` | 一个纯星形;`auto` 解析成 `star`,40 个叶子被捆成四列 |

## 趣味图(examples/fun/)

手工摆的原生文档:`view.layout` 是 `geo`,所以存的坐标就是画面。打开它们随便走走;先当笑话,
再当演示。

| 文件 | 笑点 | 展示什么 |
| --- | --- | --- |
| `owl.gtui` | 怎么画猫头鹰:两步简单的,加"最难的部分" | `single` / `double` / `heavy` 框、一个跟着成员框的区域、两个边注释、一条虚线侧枝 |
| `left-pad.gtui` | 一个应用、四个包、一个没发布的包、23 分钟的宕机 | 围着依赖塔的区域、四个边注释、虚线关联边、一个粗线框当后果 |
| `git-graph.gtui` | 一段带功能分支、热修和合并提交的小历史 | 三个分组(`main` / `feature` / `hotfix`),squash merge 和缺失 changelog 的注释 |
| `unix.gtui` | 一切都是 Unix,简写版 | 一棵纯树,钉住 `down` / `up` 端口,每条血脉一种颜色 |
| `invaders.gtui` | 用终端画一只太空入侵者 | 46 个 `1x1` 的 `none` 框当像素,逐文字上色,完全没有边 |
| `uk-dynasties.gtui` | 从 Wessex 到 Windsor 的英格兰/不列颠王室,1603 年苏格兰 Stewart 线并入 | 17 个 `point` 节点,一行标签:诺曼底分到 Blois 和 Plantagenet,Plantagenet 分到 Lancaster 与 York,两支又汇入 Tudor;Commonwealth 打断 Stuarts;没有边标签 |
| `hongloumeng.gtui` | 红楼梦:两府贾家,薛 / 史 / 林亲戚和主要仆役 | 30 个文字节点(名字就是节点)和 32 条无向边,按血亲、婚姻、爱情、仆役、姻亲分成五个配色分组 |
| `dynasties.gtui` | 从夏到今天的中国朝代,包括新(王莽)、玄汉、南明、短短的三国 / 南朝 / 北朝 / 五代各线,以及战国齐 / 楚 / 燕 / 韩 / 赵 / 魏 | 50 个内容节点:44 个 `point` 政权,旁边放一行中文标签,再加六个战国的 `none` 单行标签,组成自上而下的 DAG:政权并立处分支、统一处汇合(六个战国从东周分出、汇入秦);没有边标签 |
| `dynasties2.gtui` | 再来一版中国朝代,每个政权都直接写在节点上 | 52 个框 / 61 条边,手工摆的(`geo`):和 `dynasties.gtui` 同一个自上而下 DAG,另加一个标题框和一个提示框 |
| `links.gtui` | 一页链接,而不是一张图 | 五个 `none` 框、没有边:`https`、`mailto`、本地媒体、文件、另一个 `.gtui` 和 README;用 `Enter` / `gf` 打开 |

## 真实图(examples/real/)

这些不是编的:每一张都直接来自手边的真实数据,文件头记着来源与重新生成的方法。

| 文件 | 数据来源 | 规模 |
| --- | --- | --- |
| `modules.dot` | 本 crate 自己的模块依赖(源码里的 `crate::xxx` 引用) | 11 个模块,18 条边 |
| `cargo-deps.dot` | 本 crate 的 `Cargo.lock`,只看一层 | 21 个包,18 条边 |
| `schema.dot` | 本机 Elisa 音乐播放器数据库的外键(`pragma foreign_key_list`) | 12 张表,8 个外键 |
| `systemd.dot` | 本机 `dbus.socket` 的反向依赖(`systemctl show` 的 Requires / Wants / After) | 21 个 unit,51 条边 |
| `tcp.dot` | TCP 连接状态机(RFC 793) | 11 个状态,26 条有向边 |
| `tcp.gtui` | 上面那张的原生版:手工摆的坐标,所有节点都是纯文字(`none`),`pair` 边是虚线,三个边注释和七个自环 | 11 个节点,26 条边 |
| `hangzhou-metro.gtui` | 杭州地铁图,用 `local/bench/gen_hangzhou_metro.py` 从 `metro-tui` 的 `docs/hangzhou-subway.dot`(OSM 数据)重新生成:真实站点位置,每条线一个分组,13 个换乘站标成枢纽,`geo` 布局,所以地理就是画面 | 262 个站,299 段,13 个分组 |

前六个文件都在 11 到 21 个节点之间,不是巧合:这大概就是终端画得下、人看得清的上限。
`hangzhou-metro.gtui` 是故意的例外:一张真实的 262 站地图,留着做平移/缩放与 LOD 实验,
不是用来读的。再大(比如把 `Cargo.lock` 的传递依赖全展开,一百多个包)就成一团乱麻,GUI
也救不了,那不是终端的问题。

`tcp.dot` 里那些"原地不动"的动作(RFC 图里的重传、超时、重发 FIN)画成自环:从一侧出、
从相邻一侧回,单独放在橙色虚线的分组里。以前放在 `tcp.gtui` 图例里的状态迁移说明现在挪到了
这里,因为面板已经移除(v9),文件里也不再带它们:

| 迁移 | 原因 |
| --- | --- |
| CLOSED -> LISTEN | 被动打开 |
| CLOSED -> SYN-SENT | 主动打开 |
| LISTEN -> SYN-RECEIVED | 收到 SYN |
| SYN-SENT -> ESTABLISHED | 收到 SYN+ACK |
| SYN-RECEIVED -> LISTEN | 收到 RST(被动侧) |
| ESTABLISHED -> FIN-WAIT-1 | `close` |
| FIN-WAIT-1 -> CLOSING | 收到 FIN |
| ESTABLISHED -> CLOSE-WAIT | 收到 FIN |
| LAST-ACK -> CLOSED | 收到 ACK |
| TIME-WAIT -> CLOSED | 2MSL 到期 |
| FIN-WAIT-1 <-> CLOSE-WAIT | 两种角色 |
| SYN-SENT -> SYN-SENT, SYN-RECEIVED -> SYN-RECEIVED, ESTABLISHED -> ESTABLISHED, FIN-WAIT-1 -> FIN-WAIT-1, CLOSING -> CLOSING, LAST-ACK -> LAST-ACK, TIME-WAIT -> TIME-WAIT | 重传(独立的自环分组) |
