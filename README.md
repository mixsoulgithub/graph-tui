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

![demo](https://raw.githubusercontent.com/mixsoulgithub/graph-tui/main/assets/gtui-example01.gif)

*`examples/real/tcp.gtui` (RFC 793): `F` to fit, walk the cursor, `E` for neighbours, `a` to
toggle the edge notes.*

## Install

```bash
cargo install graph-tui
brew install mixsoulgithub/tap/graph-tui
```

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
select, right click for a menu, wheel to zoom). The keyboard is vim-flavoured: `hjkl`, `i` to
edit or create, `:` for verbs, `/` for search, `?` for the key list, digits for counts (`10l`).
Keys are rebindable per user — see [docs/config.md](docs/config.md) (`graph-tui --print-config`
prints a full commented config).

## Features

- **Routing**: orthogonal edges that snap to border ports, keep clear of boxes, jog when the
  ports are off by one and break once where two edges cross.
- **Looks**: six node styles (`point` / text-only `none` / single / rounded / double / heavy),
  per-group and per-element colours, line styles, rounded or sharp corners, legends.
- **Layouts**: `geo`, `relax`, `force` and a layered (Sugiyama-lite) pass; infinite canvas,
  pan/zoom/fit, layers and a neighbour view.
- **Editing**: text (multi-line, inline bold/italic), edge notes bound to an edge, styles,
  waypoints, cut/paste, snapshot undo, and `Ctrl+G` to hand the prompt to `$EDITOR`.
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
