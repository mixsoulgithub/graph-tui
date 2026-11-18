English | [中文](README.zh.md)

# graph-tui

[![crates.io](https://img.shields.io/crates/v/graph-tui.svg)](https://crates.io/crates/graph-tui)
[![license](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![CI](https://github.com/mixsoulgithub/graph-tui/actions/workflows/ci.yml/badge.svg)](https://github.com/mixsoulgithub/graph-tui/actions/workflows/ci.yml)

A renderer and an editor of graph in terminal. 

```bash
cargo run ./examples/real/systemd.gtui ./examples/fun/dynasties.gtui ./examples/fun/hongloumeng.gtui ./examples/limits/star40.dot ./examples/limits/k12.gtui ./examples/real/cargo-deps.gtui ./examples/limits/grid20.dot ./examples/fun/left-pad.gtui ./examples/fun/owl.gtui ./examples/fun/uk-dynasties.gtui ./examples/fun/unix.gtui ./examples/real/hangzhou-metro.gtui ./examples/real/modules.gtui  ./examples/real/tcp.gtui ./examples/styles-all.gtui
```
![demo3](https://raw.githubusercontent.com/mixsoulgithub/graph-tui/main/assets/gtui-example03.gif)

## Install

Unix like:  
```bash
curl -LsSf https://raw.githubusercontent.com/mixsoulgithub/graph-tui/main/install.sh | sh
cargo install graph-tui
```

Windows:
`.zip` on the [releases page](https://github.com/mixsoulgithub/graph-tui/releases). 

## Quick start

```bash
graph-tui examples/metro.dot              # import .dot, first save writes a .gtui next to it, NOT edit dot in place, but can export.
graph-tui --export - examples/metro.dot   # one true-colour ANSI frame, then exit
graph-tui --export out.svg examples/real/tcp.gtui
graph-tui tutor                           # built-in editable walkthrough (English)
```

## Interaction

The keyboard is vim-flavoured, also friendly to the mouse. See [Keys and interaction](docs/keybindings.md). 
Keys are rebindable, see [docs/config.md](docs/config.md) 

## Features

- **Looks**: six node styles, per-group and per-element colours, line styles, rounded or sharp corners.
- **Layouts**: Automatically draw the orthogonal edges that snap to border ports in infinite canvas. 
- **Editing**: use `i` to edit text in a box or create a box. `Ctrl+G` to edit in
  `$EDITOR`. Undo(`u`/`ctrl+z`) and redo(`ctrl+r`) is supported.
- **Interop**: import `.dot` / JSON Canvas, export `.svg` / `.png` / `.canvas` / lossy `.dot`,
  and drive everything from the `headless` JSON protocol.

## Docs

- [Usage: CLI, rendering, layouts](docs/usage.md)
- [Native format `.gtui`](docs/format.md)
- [Headless protocol](docs/headless.md)
- [Bundled examples](docs/examples.md)
- [Known trade-offs](docs/limits.md)

## License

MIT, see [LICENSE](LICENSE).
