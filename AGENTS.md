# Repository rules

These rules are for agents working in this repository and take precedence over the
global defaults (`~/.omp/agent/AGENTS.md`) wherever the two disagree.

## Language

- Code comments are English, ASCII. The only exceptions are box-drawing glyphs in
  comments that describe glyphs (`╭─╮`), and quoted sample data such as the CJK
  labels used by `examples/`.
- Docs are English only: `README.md`, `docs/*.md`, `CHANGELOG.md`, `RELEASING.md`,
  `--help` output and the generated config text (`--print-config`,
  `--init-config`).
- `README.zh.md` is a full translation of `README.md` and must be kept in sync with
  it, heading by heading.
- Chinese stays where it is data rather than documentation: UI strings picked by
  `view.lang` / `--lang`, tips, menus, assertion messages, and the CJK labels in
  `examples/*.gtui`. Do not translate or remove those.
- The default UI language is `en`; `--lang zh`, `view.lang` or a `zh` locale env
  switch it to Chinese.

## Checks before committing

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --color never -- -D warnings`
- `cargo test --color never`

## Commits

- Commit only when asked; never push unless asked explicitly.
