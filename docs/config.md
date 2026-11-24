# Config: key bindings and preferences

`graph-tui` reads one optional INI file. It is **not** discovered next to the data file —
only the three places below, so opening someone else's document never picks up their settings.

```text
1. --config <path>                      (a missing explicit file is an error)
2. $GRAPH_TUI_CONFIG
3. $XDG_CONFIG_HOME/graph-tui/config.ini   (fallback: ~/.config, Windows: %APPDATA%)
```

`--no-config` ignores all of them and runs with the built-in defaults (good for CI and for
recovering from a broken file).

## Commands

```bash
graph-tui --print-config                 # the full default file (prefs + every key)
graph-tui --init-config                  # write it to the default path
graph-tui --init-config /tmp/g.ini       # ... or to a given path
graph-tui --init-config --force          # overwrite an existing file
graph-tui --config /tmp/g.ini g.gtui     # use a specific file
graph-tui --no-config g.gtui             # ignore every file
```

The file **overlays** the built-ins: write only what you want to change. `--print-config`
prints a complete snapshot, so copying it verbatim gives you an explicit, self-contained file.

## Format

```ini
# graph-tui config
[prefs]
history = 200          # undo depth
lang = en              # zh | en
cursor = real          # real (terminal cursor) | drawn (painted into the picture)
mouse = true           # false = ignore every mouse event
mouse_edges = true     # false = a right-drag on a box pans instead of creating an edge
edge_color = auto      # auto (group colour) | #rrggbb (one ink for every uncoloured edge)
text_color = auto      # auto (box/edge colour) | #rrggbb (one ink for every uncoloured text)
opener = auto          # auto | <command> for links (usually nothing to set, see below)

[links]
custom = my-reader     # a `custom://...` link opens with `my-reader` (target appended last)

[keys]
h = move_down          # rebind one key
x = none               # unbind a default
ctrl+g = edit_text_external # any action name (see below)
```

Rules:

- `#` starts a comment; blank lines are ignored; only `[prefs]`, `[keys]` and `[links]` exist
  (anything else is an error).
- `key = value`, split at the **last** `=` so `= = zoom_in` works.
- A key may appear only once — duplicates are an error, not last-wins.
- An unknown action is an error that lists every valid name.
- Any parse error prints `path:line: message` and exits non-zero. Fix the line, or start
  with `--no-config`.
- Only **normal mode** is rebindable. Inside the prompt line (`:text`), the `:` command
  line, `/` search, menus and the `?` overlay, keys stay hard-coded: those are text-editing
  semantics and rebinding them only locks you out.
- The count prefix (`10` + `l`) and mouse gestures are not rebindable; mouse has an on/off
  switch instead (`mouse` for everything, `mouse_edges` for the right-drag edge gesture).

### Key names

| Form | Meaning |
| --- | --- |
| `e`, `E`, `:`, `+`, `=` | a single character, case-sensitive (`e` and `E` are different keys) |
| `ctrl+g`, `alt+x` | modifiers; `ctrl+` and `alt+` may be combined |
| `left` `right` `up` `down` | arrow keys |
| `home` `end` `pageup` `pagedown` `insert` `delete` | navigation keys |
| `enter` `esc` `tab` `space` `backspace` | named keys |
| `f1` .. `f12` | function keys |

`shift+` is rejected for single characters: write the shifted character instead
(`E`, `?`, `:`). Terminals report `Shift+e` as `E` anyway.

### Preferences

| Key | Values | Default |
| --- | --- | --- |
| `history` | undo depth, 1..100000 | `200` |
| `lang` | `zh` / `en`; sets the UI language and overrides a document's `view.lang` | `en` |
| `cursor` | `real` / `drawn` | `real` |
| `mouse` | `true` / `false` | `true` |
| `mouse_edges` | `true` / `false` (right-drag from a box creates an edge; `false` makes it pan) | `true` |
| `lane_gap` | columns between boxes in one layer (1-64) | `3` |
| `layer_gap` | rows between layers (1-64) | `3` |
| `margin` | blank cells left around the picture (0-256) | `0` |
| `render` | `unicode` / `ascii` glyphs; `ascii` also forces `^ v < >` arrowheads | `unicode` |
| `edge_color` | `auto` (group colour) / `#rrggbb` | `auto` |
| `text_color` | `auto` (box / edge colour, else bright) / `#rrggbb` | `auto` |
| `opener` | `auto` / a command line used to open links (usually leave it alone) | `auto` |

`edge_color` / `text_color` only apply when the document leaves a colour unset; a per-element
colour in `.gtui` wins. Setting both to `#ffffff` gives a monochrome white picture, handy for
screenshots and low-contrast terminals. In this file `#` followed by six hex digits is a **value**,
not a comment; a comment needs any other `#` (for example `edge_color = #ffffff   # white`).

Per-document view state (`layout`, `node_labels`, `edge_labels`, zoom, centre, focus) is **not**
here: it lives in the `.gtui` file's `view` object, so there is only one source of truth.

### Links you invent: `custom://`

`custom` in the sample above is just a name you choose. A target of the form `scheme://…` is a
link even when graph-tui does not know the scheme, and `[links]` maps that name to a command:
`custom = my-reader` means "a `custom://…` target opens with `my-reader`". Use whatever name fits
your files — `notes = /usr/bin/notes` handles `notes://…`, `jira = firefox` handles `jira://…`.
The same applies to schemes other tools define (`vscode://…`): without a `[links]` entry they are
still links, they just say `no handler for scheme vscode` and run nothing.

The value is a command line: the first word is the program, the rest are its options, and the
**raw target is appended as the last argument** (`custom = my-reader --wait` runs
`my-reader --wait custom://x`). No shell is involved, so a target can never inject a command. The
command runs **blocking with the terminal suspended** (a reader or TUI can take the screen; a GUI
command should detach itself), and a non-zero exit is reported on the status line.

A position suffix (`custom://x:10,10`) is parsed but not applied in this version; the target is
opened as-is and the status line says so.

### Opening links

Usually there is nothing to configure. `Enter` / `gf` on a link runs the system opener, chosen in
this order: `[prefs] opener`, `$OPENER`, then `auto` -- an `opener` helper on `PATH` when one is
installed, else `xdg-open` (Linux), `open` (macOS) or `explorer` (Windows). It follows the same
command-line rule as `[links]` (`opener = firefox --new-window`). When the program is missing, the
status line says `cannot run xdg-open: ...` and nothing else happens: no hang, no background
process.

Only URLs, `mailto:` and local files without a position reach the opener. Graph links and
positioned locals are handled inside the TUI; see [keybindings.md](keybindings.md) for that
routing.

#### When the command is the `opener` helper

If the chosen command is called `opener`, it is asked first with `opener --which -- <path>`:
`edit` runs it with the terminal suspended (so `nvim` can take over and graph-tui restores
afterwards), anything else is spawned detached. A custom opener that needs the terminal has to
suspend/restore it itself.

## Actions

A `[keys]` line is `key = action`, where `action` is one of the names the program uses
internally. `graph-tui --print-config` ends with the full list of names and a one-line
description for each (the `[keys]` lines above it carry the default keys), and
`graph-tui --init-config` writes the same file for you to edit. [keybindings.md](keybindings.md)
says what the keys do.

`toggle_edge_labels` and `confirm_selection` have no default key; bind them in `[keys]` if you want
them. `Ctrl+G` inside the prompt line is hard-coded: it is not an action and cannot be rebound.
