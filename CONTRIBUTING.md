# Contributing to rcirc

rcirc is a Rust port of [Logisim Evolution](https://github.com/logisim-evolution/logisim-evolution) (GPL-3.0-or-later).
The Java reference lives (untracked) in `reference/logisim-evolution`; port **semantics**, not line by line.

## Crate layout

| Crate | Role |
|---|---|
| `logisim-core` | `Value`/`BitWidth`, `Location`/`Direction`, `Component` trait, `Circuit` (nets + event-driven simulator) |
| `logisim-components` | Component libraries, one module each, registered in `lib.rs` |
| `logisim-format` | `.rcirc` file format (serde + RON) |
| `logisim-app` | `rcirc` egui desktop app |

## Adding a component library

1. Create `crates/logisim-components/src/<lib>.rs` (or `<lib>/mod.rs`).
2. Expose `pub fn library() -> Library` listing each part as `PartDesc { name, make }` (default options).
3. Add `<lib>` to the single `libraries!(...)` line in `lib.rs`. That is the only shared-file edit.

### Component contract (`logisim_core::Component`)
- `ports()` returns ports for an **east-facing** component at origin (0,0); the circuit rotates them
  (`Direction::rotate`). By Logisim convention the output is at (0,0) and inputs are to the left (negative x).
- Port 0 is the primary output for gates; document your port order at the top of the file.
- `propagate(ctx)` reads inputs with `ctx.port_value(i)` / `ctx.is_port_connected(i)` and drives outputs with
  `ctx.set_port(i, value, delay)` (delay >= 1 tick; gates use `gates::DELAY`). Never read time or other components.
- Keep state (flip-flop bits, memory) inside the component struct; use `tick()` for clocked behavior
  (see `wiring::Clock`). Edge detection = remember the previous clock value in your struct.
- Options/attributes are plain Rust: a `FooOptions` struct with `Default` matching Logisim defaults and
  builder-style setters (see `gates::GateOptions`). Use enums, not strings. Serialization of options is
  `logisim-format`'s job later; derive `Clone`/`Debug` so it can be added.
- Grid: positions are integer pixels on a 10-unit grid; keep ports on multiples of 10.
- Values: use `Value`/`BitWidth`; X (unknown) and E (error) must behave as in Java `Value`. Width mismatch is
  reported by the circuit, don't special-case it.

## Where to port from (`reference/.../src/main/java/com/cburch/logisim/std/`)

| Library | Java path |
|---|---|
| Gates (done) | `gates/` |
| Wiring | `wiring/` |
| Plexers | `plexers/` |
| Arithmetic | `arith/` |
| Memory | `memory/` |
| Input/Output | `io/` |
| Base (poke/text/etc.) | `base/` |
| TTL 74xx | `ttl/` |
| HDL/TCL/BFH/SoC | `hdl/`, `tcl/`, `bfh/`, `../soc/` (low priority) |

Skip painting/HDL-generator/attribute-UI code for now; port propagate logic and port layout only.

## Tests (required)

- Truth tables for combinational parts; behavior tests (set/hold/clock edges/reset) for sequential parts.
- Build small circuits with `Circuit`, `Pin` (its `handle()` changes values; call `circuit.refresh(id)`).
- `cargo build`, `cargo test` and `cargo clippy --all-targets` must be clean. Run `cargo fmt`.

## Workflow

- Branch `port/<lib>` (e.g. `port/plexers`) from `rust`; open a PR into `rust`.
- Small commits; avoid editing shared files other than the one `libraries!` line.
