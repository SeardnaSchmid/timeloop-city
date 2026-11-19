# City Timeloop

Deterministische Stadtsimulation in Rust. Architektur und Plan:
[Ages of Echo – Simulationsarchitektur](https://claude.ai/artifact/19Ga4YdpNxdwEtXZWZ2ysa).

| Crate | Inhalt |
| --- | --- |
| `core` | Weltzustand (dichte Arrays), Zufall per Hash, Zustands-Hash |
| `content` | Spielinhalte als TOML (`content/data/`) |
| `sim` | Headless-Simulator (CLI) |

```bash
cargo run --release -p timeloop-sim -- --seed 1 --days 10
```

Jede Zeile ist `day <n> <hash>`. Gleicher Seed muss auf jeder Plattform
dieselben Hashes liefern; CI prüft das auf Linux (x86, ARM), macOS und Windows.

Regeln für den Core: nur Ganzzahlen, Zufall nur über `rng`, Iteration nur nach ID.
