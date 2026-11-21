[English](README.md) | 中文

# graph-tui

[![crates.io](https://img.shields.io/crates/v/graph-tui.svg)](https://crates.io/crates/graph-tui)
[![license](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![CI](https://github.com/mixsoulgithub/graph-tui/actions/workflows/ci.yml/badge.svg)](https://github.com/mixsoulgithub/graph-tui/actions/workflows/ci.yml)

*在终端里画图。*

`graph-tui` 用无限画布、自动走线和类 vim 的操作来编辑图。原生格式是 `.gtui`(JSON),
可以读入 `.dot` 与 JSON Canvas,能导出 `png`、`svg`、文本文件,或者直接输出到终端。
它还带一套 headless JSON 协议,对 AI 友好。

```bash
cargo run ./examples/real/systemd.gtui ./examples/fun/dynasties.gtui ./examples/fun/hongloumeng.gtui ./examples/limits/star40.dot ./examples/limits/k12.gtui ./examples/real/cargo-deps.gtui ./examples/limits/grid20.dot ./examples/fun/left-pad.gtui ./examples/fun/owl.gtui ./examples/fun/uk-dynasties.gtui ./examples/fun/unix.gtui ./examples/real/hangzhou-metro.gtui ./examples/real/modules.gtui  ./examples/real/tcp.gtui ./examples/styles-all.gtui
```
![demo3](https://raw.githubusercontent.com/mixsoulgithub/graph-tui/main/assets/gtui-example03.gif)

## 安装

```bash
# Linux / macOS:
curl -LsSf https://raw.githubusercontent.com/mixsoulgithub/graph-tui/main/install.sh | sh
# 其它 Unix-like,需要 Rust 1.88+
cargo install graph-tui
```

Windows:
`.zip` 在 [Releases 页面](https://github.com/mixsoulgithub/graph-tui/releases)。

## 快速开始

```bash
graph-tui examples/metro.dot              # 导入 .dot;第一次存盘会在旁边写出 .gtui,不会原地改 dot,但可以导出
graph-tui --export - examples/metro.dot   # 打一帧真彩 ANSI 到 stdout,然后退出
graph-tui --export out.svg examples/real/tcp.gtui
graph-tui tutor                           # 内置可编辑教程(英文)
```

## 交互

键盘是 vim 风格,鼠标同样好用。见 [键位与交互](docs/keybindings.md)。
键位可以重新绑定,见 [docs/config.md](docs/config.md)。

## 功能

- **外观**:六种节点样式、按分组与按元素着色、线型、圆角或直角。
- **布局**:在无限画布上自动绘制吸附到边框端口的正交边。
- **编辑**:用 `i` 在框里改字或新建框;`Ctrl+G` 交给 `$EDITOR` 编辑。支持撤销(`u` / `ctrl+z`)
  与重做(`ctrl+r`)。
- **互通**:导入 `.dot` / JSON Canvas,导出 `.svg` / `.png` / `.canvas` / 有损的 `.dot`。

## 文档

- [用法:CLI、渲染、布局](docs/usage.md)
- [原生格式 `.gtui`](docs/format.md)
- [Headless 协议](docs/headless.md)
- [内置示例](docs/examples.md)
- [已知取舍](docs/limits.md)

## 许可证

MIT,见 [LICENSE](LICENSE)。
