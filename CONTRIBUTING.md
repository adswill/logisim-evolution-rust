# Contributing to rcirc

rcirc is a Rust port of [Logisim Evolution](https://github.com/logisim-evolution/logisim-evolution) (GPL-3.0-or-later).
The Java reference lives (untracked) in `reference/logisim-evolution`; port **semantics**, not line by line.

## Crate layout

| Crate | Role |
|---|---|
| `logisim-core` | `Value`/`BitWidth`, `Location`/`Direction`, `Component` trait, `Circuit` (nets + event-driven simulator) |
| `logisim-components` | Component libraries, one module each, registered in `lib.rs` |
| `logisim-format` | `.rcirc` file format (serde + RON) |
| `logisim-log` | Non-GUI logging, timing-diagram data and test vectors (signal history, `LogModel`/`LogWriter`, test-vector parser/evaluator) |
| `logisim-app` | `rcirc` egui desktop app |

## Adding a component library

1. Create `crates/logisim-components/src/<lib>.rs` (or `<lib>/mod.rs`).
2. Expose `pub fn library() -> Library` listing each part as `PartDesc { name, make }` (default options).
3. Add `<lib>` to the single `libraries!(...)` line in `lib.rs`. That is the only shared-file edit.

### Component contract (`logisim_core::Component`)
- `ports()` returns ports for an **east-facing** component at origin (0,0); the circuit rotates them
  (`Direction::rotate`). By Logisim convention the output is at (0,0) and inputs are to the left (negative x).
- `Component::tunnel_name` / `pull_value` are for Tunnel/Pull Resistor only. Bidirectional parts (splitters) use
  `PortKind::InOut` and `ctx.port_value_others(i)` so they never read their own drive.
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

### Options and saving (required for every part)

Parts are saved as `library` name + `part` name + `at` + `facing` + an **option map** (`OptionMap`, string keys to
`Bool`/`Int`/`Str`). Implement both on your `Component`:

```rust
fn options(&self) -> OptionMap {
    OptionMap::new().with("width", self.width.get()).with("mode", OptionMap::name_of(self.mode, &MODES))
}
fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
    m.reject_unknown(&["width", "mode"])?;                       // typo protection
    if let Some(v) = m.int("width", 1, 64)? { self.width = BitWidth::new(v as u8); }
    self.mode = m.choice("mode", &MODES)?.unwrap_or(self.mode);  // absent key = keep default
    Ok(())
}
```

Rules:
- Keys are `lower_snake_case`; enums are saved as lowercase names via a `const MODES: [(&str, Mode); N]` table.
- `options()` must return **every** option (so a saved file is complete); `set_options` accepts absent keys.
- `set_options` is called on a fresh `PartDesc::make()` part before `ports()` is read, so port layout may depend on options.
- Validate ranges (`m.int(key, lo, hi)`), never panic on bad input; return `Err(message)`.
- The registry `name` of a `PartDesc` must equal what the file stores as `part`. Never rename a released part
  (it breaks saved files); add new names instead.
- Add a test that `options()` → `set_options()` round-trips (see `every_part_round_trips_its_options` in `wiring.rs`).
  Parts with no options need nothing (defaults are empty).

### Option metadata (for the attribute panel)

Besides `options()`/`set_options()`, give the editor the description of each option by overriding
`Component::option_specs() -> Vec<OptionSpec>`:

```rust
fn option_specs(&self) -> Vec<OptionSpec> {
    vec![
        OptionSpec::int("width", "Data bits", 1, 64, 1),                       // key, label, min, max, default
        OptionSpec::choice("mode", "Mode", &["odd", "one"], "odd"),           // names must match `set_options`
        OptionSpec::boolean("negate", "Negate output", false),
        OptionSpec::string("label", "Label", ""),
    ]
}
```

- Cover **exactly** the keys of `options()` (the attribute panel shows these as widgets: number fields that accept
  decimal / `0x` hex / `0b` binary, checkboxes, drop-downs, text boxes). Empty specs = no options.
- `Int` ranges must be what `set_options` accepts. Add the test below to your library module; it checks that
  spec keys match `options()` and that every choice name and the min/max of every integer are accepted:

```rust
#[test]
fn option_specs_match_options() {
    for p in &library().parts {
        logisim_core::validate_specs(&mut *(p.make)()).unwrap_or_else(|e| panic!("{e}"));
    }
}
```

### Painting parts (`crates/logisim-app/src/paint/`)

Each library has one module `paint/<lib>.rs` exposing `pub fn painter(part: &str) -> Option<&'static dyn PartPainter>`,
registered with **one line** in the `LIBRARIES` table of `paint/mod.rs`. Parts without a painter (or libraries not listed)
are drawn as a labelled box, so painters can come later. `paint/gates.rs` is the reference.

```rust
pub trait PartPainter {
    fn paint(&self, g: &Gfx, v: &PartView);                         // draw, in east-facing local coordinates
    fn bounds(&self, v: &PartView) -> (f32, f32, f32, f32);          // local (min x, min y, max x, max y) for hit tests
}
```

- `Gfx` does rotation, position and zoom: `g.line(a, b, colour)`, `g.poly(&pts, closed, fill, colour)`, `g.circle`,
  `g.rect`, `g.text(at, anchor, "txt", size, colour)` (text is never rotated), `g.bubble(at)`; colours come from `g.theme`.
  Coordinates are Logisim pixels; the part origin (0,0) is where port 0 usually sits.
- `PartView` gives the placed part's `def` (read options with `v.int/boolean/string(key, default)`), its `ports` (local),
  `port_vals` (live net value per port, `None` when not simulating) and `port_color(theme, i)` for wire stubs.
  Draw the port stubs yourself so they match the live colours; the editor adds the port dots and the selection box.
- Keep painting pure and cheap (no allocation-heavy work per frame) and never panic on odd options.
- Verify visually: run `cargo run -p rcirc --release -- --example`, place your part from the toolbox, rotate it with `R`.

## File format (`logisim-format`)

`.rcirc` is RON: `ProjectFile { format_version, metadata, circuits, main }`. A circuit is `components`
(`library`, `part`, `at`, `facing`, `options`), `wires` (`a`, `b`) and an optional `interface` (ports it exposes
when used as a subcircuit). Subcircuit instances use `library: "Circuit"` and `part: "<circuit name>"`.
`ProjectFile::instantiate(name)` builds a simulatable `Circuit`; each subcircuit instance gets its own state.
Bump `FORMAT_VERSION` only for incompatible changes; new optional fields use `#[serde(default)]`.
Tests for your library do not need the file format, but a round trip through `ComponentDef::new(lib, part, x, y)` +
`ProjectFile::to_ron/parse/instantiate` is a good extra check.

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
