[English](README.md) | 中文

# graph-tui

[![crates.io](https://img.shields.io/crates/v/graph-tui.svg)](https://crates.io/crates/graph-tui)
[![license](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![CI](https://github.com/mixsoulgithub/graph-tui/actions/workflows/ci.yml/badge.svg)](https://github.com/mixsoulgithub/graph-tui/actions/workflows/ci.yml)

终端里的图渲染器和小编辑器。给它一张图,它就把图绘制成一格一格的字符:正交带圆角的框线边、
两条边交叉处一条从另一条下方穿过、每个分组一种颜色。文档是原生的 `.gtui` 文件(JSON,
整数单元格坐标);`.dot` 与 JSON Canvas 只作导入。headless JSON 协议让它能由脚本调用。

详细文档有中文版(见下方链接);英文版是源。

![演示](https://raw.githubusercontent.com/mixsoulgithub/graph-tui/main/assets/gtui-example01.gif)

*`examples/real/tcp.gtui`(RFC 793):`F` 铺满,光标逐个移动,`E` 查看邻居,
`a` 切换边注释。*

## 安装

```bash
cargo install graph-tui
brew install mixsoulgithub/tap/graph-tui
```

预编译包(`graph-tui-<target>.tar.gz` 与 `.sha256`,Windows 为 `.zip`)在
[Releases 页面](https://github.com/mixsoulgithub/graph-tui/releases)。从源码构建:
`cargo build --release`(Rust 1.88+)。

## 快速上手

```bash
graph-tui examples/metro.dot              # 导入 .dot,首次保存会在旁边写出 .gtui
graph-tui --dump examples/metro.dot       # 输出一帧真彩 ANSI 后退出
graph-tui --export out.svg examples/real/tcp.gtui
```

## 交互

所有操作都能用鼠标完成(单击、在节点上拖动来移动、在空白处拖动来选择、右键打开菜单、
滚轮缩放)。键盘是 vim 风格:`hjkl`,`i` 编辑或新建,`:` 执行动词,`/` 搜索,`?` 查看键位表,
数字作为计数(`10l`)。键位可以按用户重新绑定,见 [docs/config.md](docs/config.md)
(`graph-tui --print-config` 打印一份带注释的完整配置)。

## 特性

- **走线**:正交边吸附到边框端口、避开框体、端口差一格时使用 jog,两条边交叉处只断开一格。
- **外观**:六种节点样式(`point` / 只有文字的 `none` / single / rounded / double / heavy)、
  按分组和按元素的颜色、线型、圆角或锐角拐角、图例。
- **布局**:`geo`、`relax`、`force` 与一遍分层(Sugiyama-lite);无限画布、平移/缩放/铺满、
  图层和邻居视图。
- **编辑**:文字(多行、行内粗体/斜体)、绑定在边上的边注释、样式、折点、剪切/粘贴、快照撤销,
  以及用 `Ctrl+G` 把提示行交给 `$EDITOR`。
- **互通**:导入 `.dot` / JSON Canvas,导出 `.svg` / `.png` / `.canvas` / 有损 `.dot`;
  一切都能通过 `headless` JSON 协议驱动。

## 文档

- [用法:CLI、渲染、布局](docs/usage.zh.md)
- [键位与交互](docs/keybindings.zh.md)
- [配置:键位与偏好](docs/config.zh.md)
- [文档格式 `.gtui`](docs/format.zh.md)
- [Headless 协议](docs/headless.zh.md)
- [内置示例](docs/examples.zh.md)
- [已知取舍](docs/limits.zh.md)

## 许可

MIT,见 [LICENSE](LICENSE)。
