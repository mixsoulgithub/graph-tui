English | [中文](README.zh.md)

# graph-tui

[![crates.io](https://img.shields.io/crates/v/graph-tui.svg)](https://crates.io/crates/graph-tui)
[![license](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![CI](https://github.com/mixsoulgithub/graph-tui/actions/workflows/ci.yml/badge.svg)](https://github.com/mixsoulgithub/graph-tui/actions/workflows/ci.yml)

A graph renderer and a small editor for the terminal. Give it a graph and it draws a grid of
characters: orthogonal box-drawing edges with rounded corners, crossings where one edge passes
under the other, one colour per group. Documents are native `.gtui` files (JSON, integer cell
coordinates); `.dot` and JSON Canvas are imports. A headless JSON protocol makes it usable from
scripts.

```bash
graph-tui ./examples/real/systemd.gtui ./examples/fun/dynasties.gtui ./examples/fun/hongloumeng.gtui ./examples/limits/star40.dot ./examples/limits/k12.gtui ./examples/real/cargo-deps.gtui ./examples/limits/grid20.dot ./examples/fun/left-pad.gtui ./examples/fun/owl.gtui ./examples/fun/uk-dynasties.gtui ./examples/fun/unix.gtui ./examples/real/hangzhou-metro.gtui ./examples/real/modules.gtui  ./examples/real/tcp.gtui ./examples/styles-all.gtui
```
![demo3](https://raw.githubusercontent.com/mixsoulgithub/graph-tui/main/assets/gtui-example03.gif)

*`examples/real/tcp.gtui` (RFC 793): `F` to fit, walk the cursor, `E` for neighbours, `a` to
toggle the edge notes.*

## Install

```bash
curl -LsSf https://raw.githubusercontent.com/mixsoulgithub/graph-tui/main/install.sh | sh
cargo install graph-tui
brew install mixsoulgithub/tap/graph-tui
conda install -c https://mixsoulgithub.github.io/conda-channel graph-tui
```

The `curl` installer needs no sudo: it verifies the release sha256, installs `graph-tui` into
`~/.local/bin` (override with `--dir`), and accepts `--version <v>`. Run it through `sh -s --` to
pass options: `curl -LsSf .../install.sh | sh -s -- --version 0.14.0`.

Pre-built archives (`graph-tui-<target>.tar.gz` + `.sha256`, Windows is `.zip`) are on the
[releases page](https://github.com/mixsoulgithub/graph-tui/releases). From source:
`cargo build --release` (Rust 1.88+).

## Quick start

```bash
graph-tui examples/metro.dot              # import .dot, first save writes a .gtui next to it
graph-tui --dump examples/metro.dot       # one true-colour ANSI frame, then exit
graph-tui --export out.svg examples/real/tcp.gtui
```

## Interaction

Everything is reachable with the mouse (click, drag on a node to move, drag on empty space to
select, drag with the right button or middle button on empty space to pan, drag with the right
button **from a box** to link it, right click for a menu, wheel to zoom). The keyboard is
vim-flavoured: `hjkl`, `i` to
edit in place or create, `S` / `M` to split and merge boxes, `y` / `d` / `p` for the picture
clipboard, `w` / `b` to jump to the next box or edge, `:` for verbs, `/` for search, `?` for the
key list, digits for counts (`10l`).
Keys are rebindable per user — see [docs/config.md](docs/config.md) (`graph-tui --print-config`
prints a full commented config).

## Features

- **Routing**: orthogonal edges that snap to border ports, keep clear of boxes, jog when the
  ports are off by one and break once where two edges cross.
- **Looks**: six node styles (`point` / text-only `none` / single / rounded / double / heavy),
  per-group and per-element colours, line styles, rounded or sharp corners.
- **Layouts**: `auto` reads the graph structure and picks `chain`, `tree`, `layered`, or `relax`;
  `geo`, `relax`, `force`, `tree`, `chain`, and layered passes; disconnected components are
  packed. Infinite canvas, pan/zoom/fit, layers and a neighbour isolate view.
- **Editing**: text (multi-line, inline bold/italic, `[label](url-or-path)` links opened with
  `Enter` / `gf`, including `:line,column` / `:x,y` positions and any `scheme://` routed by the
  `[links]` config section), `<|new box|>` to split a box into a row of
  boxes, edge notes bound to an edge, styles, waypoints, yank/cut/paste of picture fragments,
  split (`S`) and merge (`M`) of boxes, snapshot undo, and `Ctrl+G` to edit the cursor box in
  `$EDITOR`.
- **Tabs**: open several documents in one process (`graph-tui a.gtui b.dot`), each with its own
  view and undo history.
- **Interop**: import `.dot` / JSON Canvas, export `.svg` / `.png` / `.canvas` / lossy `.dot`,
  and drive everything from the `headless` JSON protocol.

## Docs

- [Usage: CLI, rendering, layouts](docs/usage.md)
- [Keys and interaction](docs/keybindings.md)
- [Config: key bindings and preferences](docs/config.md)
- [Document format `.gtui`](docs/format.md)
- [Headless protocol](docs/headless.md)
- [Bundled examples](docs/examples.md)
- [Known trade-offs](docs/limits.md)

## License

MIT, see [LICENSE](LICENSE).
