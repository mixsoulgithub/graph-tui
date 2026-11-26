English | [中文](README.zh.md)

# graph-tui

[![crates.io](https://img.shields.io/crates/v/graph-tui.svg)](https://crates.io/crates/graph-tui)
[![license](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![CI](https://github.com/mixsoulgithub/graph-tui/actions/workflows/ci.yml/badge.svg)](https://github.com/mixsoulgithub/graph-tui/actions/workflows/ci.yml)

*Draw graphs in your terminal.*

`graph-tui` edits graphs with infinite canvas, automatic edge routing and vim-like experience. It use a native `.gtui` json format, accept `.dot` and JSON Canvas, and can export to `png`, `svg`, text file, or just to terminal.

```bash
cargo run ./examples/real/systemd.gtui ./examples/fun/dynasties.gtui ./examples/fun/hongloumeng.gtui ./examples/limits/star40.dot ./examples/limits/k12.gtui ./examples/real/cargo-deps.gtui ./examples/limits/grid20.dot ./examples/fun/left-pad.gtui ./examples/fun/owl.gtui ./examples/fun/uk-dynasties.gtui ./examples/fun/unix.gtui ./examples/real/hangzhou-metro.gtui ./examples/real/modules.gtui  ./examples/real/tcp.gtui
```
![demo3](https://raw.githubusercontent.com/mixsoulgithub/graph-tui/main/assets/gtui-example03.gif)

## Install

```bash
# Linux / macOS:  
curl -LsSf https://raw.githubusercontent.com/mixsoulgithub/graph-tui/main/install.sh | sh
# Unix like, need Rust 1.88+
cargo install graph-tui 
```

Windows:
`.zip` on the [releases page](https://github.com/mixsoulgithub/graph-tui/releases). 

## Quick start

```bash
graph-tui tutor                           # built-in editable walkthrough (English)
graph-tui examples/org.dot     # one true-colour ANSI frame, then exit
graph-tui --export - examples/org.dot     # one true-colour ANSI frame, then exit
graph-tui --export out.svg examples/real/tcp.gtui   # export to svg
```

## Docs

- [Usage: CLI, rendering, layouts](docs/usage.md)
- [Keys and mouse](docs/keybindings.md)
- [Configuration](docs/config.md) 
- [Native format `.gtui`](docs/format.md)
- [Examples](docs/examples.md)
- [Known trade-offs](docs/limits.md)

## License

MIT, see [LICENSE](LICENSE).
