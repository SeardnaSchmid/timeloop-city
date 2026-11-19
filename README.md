# Timeloop City

Deterministic city simulation in Rust. Architecture and roadmap (German):
[Ages of Echo – Simulationsarchitektur](https://claude.ai/artifact/19Ga4YdpNxdwEtXZWZ2ysa).

| Crate | Contents |
| --- | --- |
| `core` | Event queue, lazy needs, world state (dense arrays), hash-based randomness, state hash |
| `content` | Game content as TOML (`content/data/`) |
| `sim` | Headless simulator (CLI) |

```bash
cargo run --release -p timeloop-sim -- --seed 1 --days 10
```

Each line is `day <n> <hash>`. Add `--log` to see every event, `--agents N` to
change the population:

```bash
cargo run --release -p timeloop-sim -- --agents 1 --days 2 --log
```

Golden event logs in `sim/tests/golden/` pin the exact behaviour. After an
intended change, review the diff and regenerate them with
`UPDATE_GOLDEN=1 cargo test -p timeloop-sim --test golden`.

The same seed must produce the same hashes on
every platform; CI checks this on Linux (x86, ARM), macOS and Windows.

Core rules: integers only, randomness only through `rng`, iteration only in ID order.
