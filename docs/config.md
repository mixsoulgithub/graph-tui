English | [中文](config.zh.md)

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
arrow = ascii          # ascii | arrows | triangles
history = 200          # undo depth
lang = zh              # zh | en
cursor = real          # real (terminal cursor) | drawn (painted into the picture)
mouse = true           # false = ignore every mouse event

[keys]
h = move_down          # rebind one key
x = none               # unbind a default
ctrl+g = edit_external # any action from the table below
```

Rules:

- `#` starts a comment; blank lines are ignored; only `[prefs]` and `[keys]` exist
  (anything else is an error).
- `key = value`, split at the **last** `=` so `= = zoom_in` works.
- A key may appear only once — duplicates are an error, not last-wins.
- An unknown action is an error that lists every valid name.
- Any parse error prints `path:line: message` and exits non-zero. Fix the line, or start
  with `--no-config`.
- Only **normal mode** is rebindable. Inside the prompt line (`i` / `t`), the `:` command
  line, `/` search, menus and the `?` overlay, keys stay hard-coded: those are text-editing
  semantics and rebinding them only locks you out.
- The count prefix (`10` + `l`) and mouse gestures are not rebindable; mouse has an on/off
  switch instead.

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
| `arrow` | `ascii` / `arrows` / `triangles` | `ascii` |
| `history` | undo depth, 1..100000 | `200` |
| `lang` | `zh` / `en` | `zh` |
| `cursor` | `real` / `drawn` | `real` |
| `mouse` | `true` / `false` | `true` |

Per-document view state (`layout`, `labels`, `legend`, `notes`, zoom, centre, focus) is **not**
here: it lives in the `.gtui` file's `view` object, so there is only one source of truth.

## Actions

| Action | Meaning | Default keys |
| --- | --- | --- |
| `escape` | leave the neighbour view, then clear the selection | `esc` |
| `help` | key reference overlay | `?` |
| `center` | centre the cursor box | `z`, `space` |
| `fit` | fit the focused group to the screen | `f` |
| `fit_all` | fit the whole document | `F` |
| `link` | two-phase linking (source, then target) | `e` |
| `block_select` | block selection from the cursor cell | `v`, `V`, `ctrl+q` |
| `cycle_layer` | cycle the layer filter | `L` |
| `ego_toggle` | neighbour view on/off | `E` |
| `toggle_notes` | edge notes on/off | `a` |
| `cycle_edge` | cycle the edges incident to the cursor node | `ctrl+e` |
| `leap_down` | jump half a screen down | `ctrl+d` |
| `leap_up` | jump half a screen up | `ctrl+u` |
| `jump_back` | back to the previous jump | `ctrl+o` |
| `jump_forward` | forward in the jump history | `tab`, `ctrl+i` |
| `move_left` / `move_right` / `move_up` / `move_down` | move the cursor (count prefix works) | `h` `l` `k` `j`, arrows |
| `nudge_left` / `nudge_right` / `nudge_up` / `nudge_down` | move the selected boxes by one cell | `ctrl+h` `ctrl+l` `ctrl+k` `ctrl+j` |
| `zoom_in` / `zoom_out` / `zoom_reset` | zoom | `+` `=` / `-` `_` / `0` |
| `toggle_labels` | point-node name labels | `b` |
| `search` | search / focus a group | `/` |
| `next_hit` / `prev_hit` | next / previous search hit | `n` / `N` |
| `cycle_layout` | next layout | `r` |
| `undo` / `redo` | history | `u` / `ctrl+r` |
| `cut` / `cut_box` | cut with edges / box and text only | `d` / `x` |
| `edit_or_new` | edit text, create a box, or add an edge note | `i` |
| `paste` | paste | `p` |
| `confirm_selection` | confirm the block selection | `enter` |
| `edit_text` | edit the cursor box text | `t` |
| `color_menu` / `style_menu` | colour menu / style menu | `c` / `s` |
| `command` | command line | `:` |
| `quit_hint` | remind how to quit (only `:q` quits) | `q` |

`edit_external` (`Ctrl+G` inside the prompt line) is listed for reference; it is handled by
the prompt itself and cannot be rebound.
