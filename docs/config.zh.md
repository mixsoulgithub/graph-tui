[English](config.md) | 中文

# 配置:键位与偏好

`graph-tui` 只读一个可选的 INI 文件。它**不会**在数据文件旁边自动找,只认下面三个位置,
所以打开别人的文档永远不会带上别人的设置。

```text
1. --config <path>                      (显式给的路径;文件不存在就报错)
2. $GRAPH_TUI_CONFIG
3. $XDG_CONFIG_HOME/graph-tui/config.ini   (回退:~/.config;Windows 用 %APPDATA%)
```

`--no-config` 全部忽略,用内置默认值跑(CI 和修复坏配置时好用)。

## 命令

```bash
graph-tui --print-config                 # 完整的默认文件(prefs + 所有键)
graph-tui --init-config                  # 写到默认路径
graph-tui --init-config /tmp/g.ini       # ... 或者写到指定路径
graph-tui --init-config --force          # 覆盖已有文件
graph-tui --config /tmp/g.ini g.gtui     # 用指定文件
graph-tui --no-config g.gtui             # 忽略所有配置文件
```

这个文件是**叠加**在内置默认值上的:只写想改的部分。`--print-config` 打一份完整快照,
整份抄下来就是一个显式、自包含的文件。

## 格式

```ini
# graph-tui 配置
[prefs]
arrow = ascii          # ascii | arrows | triangles
history = 200          # 撤销步数
lang = zh              # zh | en
cursor = real          # real(终端真光标) | drawn(画进画面)
mouse = true           # false = 忽略所有鼠标事件
mouse_edges = true     # false = 从框上右键拖会平移,而不是连边
edge_color = auto      # auto(跟分组色) | #rrggbb(所有没上色的边用同一种墨色)
text_color = auto      # auto(跟框/边色) | #rrggbb(所有没上色的文字用同一种墨色)
opener = auto          # auto | <命令>,用来打开链接(见下)

[links]
custom = my-reader     # 任意 `scheme://` 链接 -> 这个命令,target 作为最后一个参数追加

[keys]
h = move_down          # 改绑一个键
x = none               # 解绑一个默认键
ctrl+g = edit_text_external # 下表里的任意动作
```

规则:

- `#` 开始注释;空行忽略;只有 `[prefs]`、`[keys]` 与 `[links]` 三节(别的都报错)。
- `键 = 值`,按**最后一个** `=` 切,所以 `= = zoom_in` 能正常工作。
- 同一个键只能出现一次,重复直接报错,不是后者覆盖。
- 未知动作会报错,并列出所有合法名字。
- 任何解析错误都会打出 `path:line: message` 并以非零退出。改那一行,或者用 `--no-config` 启动。
- 只有**普通模式**能改键。提示行(`:text`)、`:` 命令行、`/` 搜索、菜单和 `?` 浮层里的键
  是写死的:那些是文本编辑语义,改绑只会把自己锁在外面。
- 数字前缀(`10` + `l`)与鼠标手势不能改绑;鼠标只有开关(`mouse` 管全部,`mouse_edges`
  管右键拖连边)。

### 键名

| 写法 | 含义 |
| --- | --- |
| `e`, `E`, `:`, `+`, `=` | 单个字符,区分大小写(`e` 与 `E` 是两个不同的键) |
| `ctrl+g`, `alt+x` | 修饰键;`ctrl+` 与 `alt+` 可以叠加 |
| `left` `right` `up` `down` | 方向键 |
| `home` `end` `pageup` `pagedown` `insert` `delete` | 导航键 |
| `enter` `esc` `tab` `space` `backspace` | 具名键 |
| `f1` .. `f12` | 功能键 |

单个字符不接受 `shift+`,直接写按了 Shift 之后的字符(`E`、`?`、`:`)。终端本来就把
`Shift+e` 报成 `E`。

### 偏好

| 键 | 取值 | 默认 |
| --- | --- | --- |
| `arrow` | `ascii` / `arrows` / `triangles` | `ascii` |
| `history` | 撤销步数,1..100000 | `200` |
| `lang` | `zh` / `en` | `zh` |
| `cursor` | `real` / `drawn` | `real` |
| `mouse` | `true` / `false` | `true` |
| `mouse_edges` | `true` / `false`(从框上右键拖会连边;`false` 改成平移) | `true` |
| `lane_gap` | 同一层相邻两框之间的列数(1-64) | `3` |
| `layer_gap` | 层与层之间的行数(1-64) | `3` |
| `margin` | 画面四周留的空格数(0-256) | `0` |
| `render` | `unicode` / `ascii` 字形 | `unicode` |
| `edge_color` | `auto`(跟分组色)/ `#rrggbb` | `auto` |
| `text_color` | `auto`(跟框 / 边色,否则亮色)/ `#rrggbb` | `auto` |
| `opener` | `auto` / 用来打开链接的命令名或路径 | `auto` |

每份文档的视图状态(`layout`、`labels`、`notes`、缩放、中心、聚焦)不在这里:
它们存在 `.gtui` 的 `view` 对象里,只有一个真值来源。

### 自定义 URI scheme

任何 `scheme://…` 形式的目标(`foo://x`、`vscode://…` 等)都是链接。核心不认识它们:
它把**小写**的 `scheme` 拿到 `[links]` 里查,然后运行配置好的命令,把**原始 target 作为
一个额外参数**追加(`my-reader foo://x`)。不经过 shell,所以 target 永远无法注入命令;
能带选项的命令写成一个字符串(`my-reader --flag`),target 仍然追加在最后。命令**阻塞运行,
终端会先挂起**(阅读器或 TUI 可以接管屏幕;GUI 命令应自己脱离),非零退出会在状态行报告。

没有 `[links]` 条目的 scheme 只提示 `no handler for scheme foo`,什么都不执行,文档也不动。
位置后缀(`foo://x:10,10`)会解析但在这个版本里不生效;target 原样打开,状态行会说明。

`opener` 决定 `Enter` / `gf` 打开链接时跑什么:配置里的命令优先,然后是 `$OPENER`,最后是
`auto`(`PATH` 里的 `opener` 助手,否则 `xdg-open` / `open` / `explorer`)。本地 `.gtui` /
`.dot` 链接永远在 graph-tui 里开成另一个标签页。命令是 `opener` 助手时,会先问它
`opener --which -- <path>`:`edit` 就挂起终端运行它(所以 `nvim` 能接管,退出后 graph-tui
恢复),`open` 只 spawn,其他就脱离运行。需要终端的自定义 opener 必须自己挂起/恢复。

`edge_color` / `text_color` 只填文档留空的部分:`.gtui` 里自带颜色的边或文字保持原样
(`:edgecolor` / `:textcolor` 改的是那些)。两个都设成 `#ffffff` 会得到一张单色白图,截图
或对比度差的终端上很好用。在这个文件里,`#` 后面正好六位十六进制是**值**,不是注释;要写注释
得用别的 `#`,例如 `edge_color = #ffffff   # white`。

## 动作

| 动作 | 含义 | 默认键 |
| --- | --- | --- |
| `escape` | 退出隔离视图,再取消选区 | `esc` |
| `help` | 键位速查浮层 | `?` |
| `center` | 把光标格挪到画面正中 | `space` |
| `fit` | 把聚焦的分组铺满画面 | `f` |
| `fit_all` | 铺满整幅图 | `F` |
| `link` | 两段式连边(先源,再目标) | `e` |
| `block_select` | 从光标格开始区块选择 | `v` |
| `cycle_layer` | 循环图层过滤 | `L` |
| `ego_toggle` | 隔离光标节点的邻居(其余全部隐藏) | `E` |
| `toggle_notes` | 边注释开关 | 无 |
| `cycle_edge` | 在光标节点的相连边之间轮选 | `ctrl+e` |
| `leap_down` | 往下跳半屏 | `ctrl+d` |
| `leap_up` | 往上跳半屏 | `ctrl+u` |
| `jump_back` | 回到上一次跳转 | `ctrl+o` |
| `jump_forward` | 在跳转历史里前进 | `tab` |
| `jump_next` / `jump_prev` | 沿画面主轴打一条射线,跳到碰到的第一个框或边(整幅图,不只是窗口)并把视图滚过去;整条线都空时取那个方向上最近的框;什么都没有就提示 | `w` / `b` |
| `move_left` / `move_right` / `move_up` / `move_down` | 移动光标(支持数字前缀) | `h` `l` `k` `j`, 方向键 |
| `nudge_left` / `nudge_right` / `nudge_up` / `nudge_down` | 把选中的框挪一格 | `ctrl+h` `ctrl+l` `ctrl+k` `ctrl+j` |
| `zoom_in` / `zoom_out` / `zoom_reset` | 缩放 | `+` / `-` / `0` |
| `toggle_labels` | 点节点名标注 | `B` |
| `search` | 搜索 / 聚焦一个分组 | `/` |
| `next_hit` / `prev_hit` | 下一个 / 上一个搜索命中 | `n` / `N` |
| `cycle_layout` | 下一个布局 | `r` |
| `undo` / `redo` | 撤销 / 重做 | `u` / `ctrl+r` |
| `cut` | 剪切选区(碰到它的边一起走) | `d` |
| `edit_or_new` | 原地改光标框的文字(没有框时新建框 / 建边注释) | `i` |
| `paste` | 粘贴 | `p` |
| `open_link` | 打开光标下的链接(`[text](target)`);有未确认的区块选区时先确认选区 | `enter` |
| `confirm_selection` | 确认区块选择(没有默认键;想要回车确认就自己绑) | 无 |
| `yank` | 复制选中的框、文字与它们的边 | `y` |
| `split_box` | 在光标行与下一行之间拆开光标框 | `S` |
| `merge_boxes` | 把选中的框合并进光标框 | `M` |
| `edit_text_external` | 用 `$VISUAL` / `$EDITOR` 改光标框文字,退出即应用 | `ctrl+g` |
| `color_menu` / `style_menu` | 颜色菜单 / 样式菜单 | `c` / `s` |
| `command` | 命令行 | `:` |
| `quit_hint` | 提醒怎么退出(只有 `:q` 退出) | `q` |
| `reload` | 从磁盘重新载入源文件(再按一次丢弃改动) | `R` |
| `tab_new` | 再开一个标签页(空白文档) | `ctrl+t` |
| `tab_next` / `tab_prev` | 下一个 / 上一个标签页 | `ctrl+n` / `ctrl+p` |
| `tab_close` | 关闭当前标签页(有未保存改动时拒绝) | `ctrl+w` |

`toggle_notes` 与 `confirm_selection` 默认没有键;想要就在 `[keys]` 里绑。
提示行里的 `Ctrl+G` 是写死的:它不是动作,也不能改绑。
