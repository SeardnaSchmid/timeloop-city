# Timeloop City

Deterministic city simulation in Rust. Architecture and roadmap (German):
[Ages of Echo – Simulationsarchitektur](https://claude.ai/artifact/19Ga4YdpNxdwEtXZWZ2ysa).

| Crate | Contents |
| --- | --- |
| `core` | Event queue, lazy needs, economy (businesses, market), world state, hash-based randomness, state hash |
| `content` | Game content as TOML (`content/data/`), JSON schema (`content/schema/`) |
| `sim` | Headless simulator (CLI) |

```bash
cargo run --release -p timeloop-sim -- --seed 1 --days 10
```

Each line is `day <n> <hash>`. Add `--log` to see every event, `--agents N` to
change the population:

```bash
cargo run --release -p timeloop-sim -- --agents 1 --days 2 --log
```

Add `--stats FILE` to write one CSV row per day (money, prices, wages, cash,
staff, stock, employment, hunger, meals):

```bash
cargo run --release -p timeloop-sim -- --days 3650 --stats stats.csv --quiet
```

The default world is a bread town: field -> mill -> bakery, 50 agents. Money is
a closed loop (never created or destroyed); prices and wages move in fixed daily
steps. `sim/tests/bread_town.rs` checks that ten years stay fed and balanced.

`content/data/world.toml` points at its schema, so editors with TOML schema
support (Taplo / Even Better TOML) autocomplete and validate it. After changing
the content structs, regenerate it with `UPDATE_GOLDEN=1 cargo test -p timeloop-content`.

Golden event logs in `sim/tests/golden/` pin the exact behaviour. After an
intended change, review the diff and regenerate them with
`UPDATE_GOLDEN=1 cargo test -p timeloop-sim --test golden`.

The same seed must produce the same hashes on
every platform; CI checks this on Linux (x86, ARM), macOS and Windows.

Core rules: integers only, randomness only through `rng`, iteration only in ID order.
