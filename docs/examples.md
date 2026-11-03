English | [中文](examples.zh.md)

# Bundled examples

These graphs deliberately push the engine to its limits:

| file | what it stresses |
| --- | --- |
| `star40.dot` | a hub with 40 leaves: one cell has only four directions, fan-out at the limit; `auto` / `tree` draw it as a fan (four columns of ten, hub centred), `61x19` instead of `527x6` |
| `k12.dot` | K12 complete graph: 66 edges crammed onto 12 points |
| `grid20.dot` | 20x20 grid, 400 points, 760 edges: density and timing |
| `chain60.dot` | a 60-long chain through `layered`: 3-cell layer height x 60 layers, stacks when the window is too short |
| `ring.dot` | a directed cycle; layered layout must break one back edge |
| `parallel.dot` | four edges on one node pair across four groups: each gets its own parallel line (no border cells left to shift into, so lane offsets do it) |
| `selfloop.dot` | self-loop: out one side, back in on an adjacent side |
| `isolated.dot` | isolated node plus one connected component |
| `components.dot` | four disconnected components |
| `inarrow.dot` | 12 incoming edges to one point: arrowheads only have four directions |
| `collinear.dot` | eight collinear nodes |
| `samepos.dot` | five nodes with identical coordinates |
| `outlier.dot` | one distant outlier expands the bounding box |
| `dupname.dot` | two nodes with the same name (one node in .dot semantics) |
| `longlabel.dot` / `cjk.dot` | very long labels and CJK label width |

They measure the edges of the engine: after projection nodes can land on the same cell, lines can
run out of room, labels may not fit — everything is still drawn (overlaps stay overlapped) and the
status line does not report these details; that space belongs to commands and messages.

## Structure profiles (examples/)

These two are small on purpose: they exist to pin down what `auto` picks and how `.dot` imports
map onto regions and ports.

| file | what it shows |
| --- | --- |
| `org.dot` | a pure tree with two `subgraph cluster_*` blocks (imported as regions) and one edge with `from_side` / `to_side`; `auto` resolves to `tree`, the frames follow their members, and the ports survive the `.dot` round-trip |
| `timeline.dot` | a pure chain; `auto` resolves to `chain` and the whole pipeline lands on one row |
| `limits/star40.dot` | a pure star; `auto` resolves to `star` and the 40 leaves are bundled into four columns |

## Real-world graphs (examples/real/)

These are not invented: each was converted directly from real data lying around, and the file
header records the source and how to regenerate it.

| file | data source | size |
| --- | --- | --- |
| `modules.dot` | this crate's own module dependencies (`crate::xxx` references in the source) | 11 modules, 18 edges |
| `cargo-deps.dot` | this crate's `Cargo.lock`, one level deep | 21 packages, 18 edges |
| `schema.dot` | foreign keys of the local Elisa music player database (`pragma foreign_key_list`) | 12 tables, 8 foreign keys |
| `systemd.dot` | reverse dependencies of the local `dbus.socket` (`systemctl show` Requires / Wants / After) | 21 units, 51 edges |
| `tcp.dot` | the TCP connection state machine (RFC 793) | 11 states, 26 directed edges |
| `tcp.gtui` | the native version of the above: hand-placed coordinates, all nodes text-only (`none`), the `pair` edge is dashed, three edge notes and seven self-loops | 11 nodes, 26 edges |
| `hangzhou-metro.gtui` | the Hangzhou metro map, regenerated from `metro-tui`'s `docs/hangzhou-subway.dot` (OSM data) with `local/bench/gen_hangzhou_metro.py`: real station positions, one group per line, 13 transfer stations flagged as hubs, `geo` layout so the geography is the picture | 262 stations, 299 segments, 13 groups |

The first six files all sit at 11–21 nodes — not a coincidence: that is about the size a terminal
can draw and a person can read. `hangzhou-metro.gtui` is the deliberate exception: it is a real
262-station map, kept for pan/zoom and LOD experiments rather than for reading. Beyond that (say,
fully expanding the transitive dependencies of `Cargo.lock`, a hundred-plus packages) it turns into
a knot, and a GUI would not help; that is not a terminal problem.

The "stay in place" actions in `tcp.dot` (retransmit, timeout, resend FIN in the RFC diagram) are
drawn as self-loops: out one side, back in on an adjacent side, in their own orange dashed group.
The state-transition explanations that used to live in `tcp.gtui`'s legend are reproduced here, since
the panel is gone (v9) and the file no longer carries them:

| transition | why |
| --- | --- |
| CLOSED -> LISTEN | passive open |
| CLOSED -> SYN-SENT | active open |
| LISTEN -> SYN-RECEIVED | got SYN |
| SYN-SENT -> ESTABLISHED | got SYN+ACK |
| SYN-RECEIVED -> LISTEN | got RST (passive side) |
| ESTABLISHED -> FIN-WAIT-1 | `close` |
| FIN-WAIT-1 -> CLOSING | got FIN |
| ESTABLISHED -> CLOSE-WAIT | got FIN |
| LAST-ACK -> CLOSED | got ACK |
| TIME-WAIT -> CLOSED | 2MSL elapsed |
| FIN-WAIT-1 <-> CLOSE-WAIT | the two roles |
| SYN-SENT -> SYN-SENT, SYN-RECEIVED -> SYN-RECEIVED, ESTABLISHED -> ESTABLISHED, FIN-WAIT-1 -> FIN-WAIT-1, CLOSING -> CLOSING, LAST-ACK -> LAST-ACK, TIME-WAIT -> TIME-WAIT | retransmit (own self-loop group) |
