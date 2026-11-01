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

[keys]
h = move_down          # 改绑一个键
x = none               # 解绑一个默认键
ctrl+g = edit_external # 下表里的任意动作
```

规则:

- `#` 开始注释;空行忽略;只有 `[prefs]` 与 `[keys]` 两节(别的都报错)。
- `键 = 值`,按**最后一个** `=` 切,所以 `= = zoom_in` 能正常工作。
- 同一个键只能出现一次,重复直接报错,不是后者覆盖。
- 未知动作会报错,并列出所有合法名字。
- 任何解析错误都会打出 `path:line: message` 并以非零退出。改那一行,或者用 `--no-config` 启动。
- 只有**普通模式**能改键。提示行(`i` / `t`)、`:` 命令行、`/` 搜索、菜单和 `?` 浮层里的键
  是写死的:那些是文本编辑语义,改绑只会把自己锁在外面。
- 数字前缀(`10` + `l`)与鼠标手势不能改绑;鼠标只有一个开关。

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

每份文档的视图状态(`layout`、`labels`、`legend`、`notes`、缩放、中心、聚焦)不在这里:
它们存在 `.gtui` 的 `view` 对象里,只有一个真值来源。

## 动作

| 动作 | 含义 | 默认键 |
| --- | --- | --- |
| `escape` | 先退出邻居视图,再取消选区 | `esc` |
| `help` | 键位速查浮层 | `?` |
| `center` | 把光标框挪到画面正中 | `z`, `space` |
| `fit` | 把聚焦的分组铺满画面 | `f` |
| `fit_all` | 铺满整幅图 | `F` |
| `link` | 两段式连边(先源,再目标) | `e` |
| `block_select` | 从光标格开始区块选择 | `v`, `V`, `ctrl+q` |
| `cycle_layer` | 循环图层过滤 | `L` |
| `ego_toggle` | 邻居视图开关 | `E` |
| `toggle_notes` | 边注释开关 | `a` |
| `cycle_edge` | 在光标节点的相连边之间轮选 | `ctrl+e` |
| `leap_down` | 往下跳半屏 | `ctrl+d` |
| `leap_up` | 往上跳半屏 | `ctrl+u` |
| `jump_back` | 回到上一次跳转 | `ctrl+o` |
| `jump_forward` | 在跳转历史里前进 | `tab`, `ctrl+i` |
| `move_left` / `move_right` / `move_up` / `move_down` | 移动光标(支持数字前缀) | `h` `l` `k` `j`, 方向键 |
| `nudge_left` / `nudge_right` / `nudge_up` / `nudge_down` | 把选中的框挪一格 | `ctrl+h` `ctrl+l` `ctrl+k` `ctrl+j` |
| `zoom_in` / `zoom_out` / `zoom_reset` | 缩放 | `+` `=` / `-` `_` / `0` |
| `toggle_labels` | 点节点名标注 | `b` |
| `search` | 搜索 / 聚焦一个分组 | `/` |
| `next_hit` / `prev_hit` | 下一个 / 上一个搜索命中 | `n` / `N` |
| `cycle_layout` | 下一个布局 | `r` |
| `undo` / `redo` | 撤销 / 重做 | `u` / `ctrl+r` |
| `cut` / `cut_box` | 带边剪切 / 只剪框和文字 | `d` / `x` |
| `edit_or_new` | 改文字、新建框,或加边注释 | `i` |
| `paste` | 粘贴 | `p` |
| `confirm_selection` | 确认区块选择 | `enter` |
| `edit_text` | 改光标框的文字 | `t` |
| `color_menu` / `style_menu` | 颜色菜单 / 样式菜单 | `c` / `s` |
| `command` | 命令行 | `:` |
| `quit_hint` | 提醒怎么退出(只有 `:q` 退出) | `q` |

`edit_external`(提示行里的 `Ctrl+G`)只是列出来备查;它由提示行自己处理,不能改绑。
