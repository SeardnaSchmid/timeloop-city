# Timeloop City

Deterministic city simulation in Rust. Architecture and roadmap (German):
[Ages of Echo – Simulationsarchitektur](https://claude.ai/artifact/19Ga4YdpNxdwEtXZWZ2ysa).

| Crate | Contents |
| --- | --- |
| `core` | Event queue, lazy needs, utility decisions with an integer formula language, map and housing, economy (businesses, market), world state, hash-based randomness, state hash |
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

Add `--stats FILE` to write one CSV row per day (money of agents, businesses and in total, prices, wages, cash,
staff, stock, employment, hunger, meals, work hours, money Gini, share of
time spent starving, rent and tenants per house, homeless, mean commute,
founded firms open / founded / closed, money held by firm owners, deaths,
births, newcomers, mean age, people in education and retired):

```bash
cargo run --release -p timeloop-sim -- --days 3650 --stats stats.csv --quiet
```

Agents decide by utility: every action (`eat`, `sleep`, `work`, `overtime`,
`wander`) has a `when` and a `score` formula in `[action.*]` of the world file,
reading needs (hunger, fatigue, safety), money, the clock and character traits
(`greed`, `work_life`, rolled per `[[population]]`). The highest allowed score
wins. To see why an agent does what it does:

```bash
cargo run --release -p timeloop-sim -- explain 7 3:08:00
```

The town is a grid (`[map]`). Every business and `[[house]]` stands on one
parcel; walking takes `travel_minutes_per_parcel` per parcel of Manhattan
distance, and work and sleep happen at the workplace and at home. Houses rent
out beds by the day; the rent rises when a full house turns people away and
falls while beds stay empty. Once a day every agent scores the houses with a
free bed against its own and against staying homeless (`[housing]` formulas),
so people move closer to work when it pays. Without a home, sleep recovers at
half the rate.

The businesses of the world file belong to the town; their surplus goes to
agents short of savings (welfare). Agents found more (`[founding]`): once a
day everyone without a firm scores founding one for each good, reading
`market.price`, `market.cost`, `market.shortage`, `market.firms` and
`market.newest_days`; by default a good must sell at 1.5 times its cost and
nobody may have started making it in the last year. The best founding of the
day happens: the founder pays the capital, builds on the free parcel nearest
home and runs the firm as CEO (works there, is never laid off, takes what the
till can pay). The firm's surplus goes to the owner after a profit tax that
joins the welfare. Any business that has been broke for a month closes (its
cash to the owner, its stock lost); a broke business may sell below cost.
Above safety, agents spend 4% a day of their savings beyond the reserve on
comforts (`[needs.comfort]`); without that, owners end up holding all the
money and demand dies.

People age (`[life]`): the first inhabitants are 16-70, everyone's lifespan
is rolled between 70 and 90. Until 18 they are in education, from 65 retired;
neither takes a job. A fifth of every wage goes into a pension fund that is
shared out daily among them. Whoever dies leaves money (after 50%
inheritance tax) and firm to their children; without children both go to the
town. The same day someone new takes the free place: mostly a child of a
living agent aged 20-50 (population and traits close to the parent's),
otherwise a newcomer, entering at 16 with no money. Childhood is not
simulated. A business stores at most four times its output target.

To watch and steer the town live, start the engine as a local server and
open the page it prints. The engine runs natively in its own thread for up to
80 years (`--years N` to change); the page shows the map as it is right now,
including founded firms (white corner, labelled with the owner), with play/pause, a speed control from 1 game minute per second up to as fast
as possible (Space, arrow keys), a history chart to jump to any point (back
via a checkpoint per year), restart with another seed, and for a clicked agent
how it scores every action. Speed never changes the outcome: the state hash
at a given game minute is the same however you got there.

```bash
cargo run --release -p timeloop-sim -- serve        # http://127.0.0.1:7878
```

To share a run as a single file instead, write a viewer and open it in a browser (no server needed).
It plays back the whole run: agents coloured by action, houses with tenants
and rent, a land-price layer, a history chart to jump around in, and details
for any agent or building you click. The frame step adapts to the length of
the run (about 4000 frames; daily or longer steps show noon); `--html-step`
sets it, `--html-days N` limits the viewer to the last N days. A whole life
of 80 years takes about 10 seconds:

```bash
cargo run --release -p timeloop-sim -- --years 80 --html map.html --quiet
```

With `--world FILE --watch` the rules are reloaded whenever the file changes,
so behaviour can be tuned while a long run is going.

The default world is a bread town: field -> mill -> bakery, 50 agents. Money is
a closed loop (never created or destroyed); prices and wages move in fixed daily
steps. `sim/tests/bread_town.rs` checks that ten years stay fed and balanced;
`sim/tests/traits.rs` checks that diligent and easygoing populations work
different hours and end up with different wealth; `sim/tests/city.rs` checks
that vacancy settles, rents stay bounded and commutes get shorter;
`sim/tests/firms.rs` checks over six seeds of 20 years that firms get founded
and survive, the town stays fed and owners hold less than half the money;
`sim/tests/life.rs` runs 80 years and checks deaths, births, newcomers and
the age structure.

`content/data/world.toml` points at its schema, so editors with TOML schema
support (Taplo / Even Better TOML) autocomplete and validate it. After changing
the content structs, regenerate it with `UPDATE_GOLDEN=1 cargo test -p timeloop-content`.

Golden event logs in `sim/tests/golden/` pin the exact behaviour. After an
intended change, review the diff and regenerate them with
`UPDATE_GOLDEN=1 cargo test -p timeloop-sim --test golden`.

The same seed must produce the same hashes on
every platform; CI checks this on Linux (x86, ARM), macOS and Windows.

Core rules: integers only, randomness only through `rng`, iteration only in ID order.
