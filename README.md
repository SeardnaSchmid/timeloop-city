# Timeloop City

Deterministic city simulation in Rust. Architecture and roadmap (German):
[Ages of Echo – Simulationsarchitektur](https://claude.ai/artifact/19Ga4YdpNxdwEtXZWZ2ysa).
Design decisions for economy, careers and needs (German):
[docs/economy-design.md](docs/economy-design.md), plan: [docs/implementation-plan.md](docs/implementation-plan.md).

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

`--report` prints one line per year (per month under two years) on how the
town is doing and why it does not grow: employment, wage, meal price, meals a
day, starvation and whether it came from no money or empty shelves, rough
sleepers, beds, town branches and houses, and what blocks new branches:

```bash
cargo run --release -p timeloop-sim -- --years 3 --report
```

`--snapshots DIR` does the same at years 1, 2, 3, 5, 10, 15, ... and writes
`periods.csv` (one row per cut, Excel-ready), the map as text
(`map_yNN.txt`) and every agent (`agents_yNN.csv`). 30 years of 100 agents
take about 15 seconds. Besides the table's columns, `periods.csv` has
`purchasing_power` (a full day's net median wage over 3.2 meals plus the
cheapest bed), the money moved during the period by kind (wages, pensions,
rent, taxes, dues, welfare, levy, inheritance, founding capital, ...) and
closed firms by reason (`closed_owner_dead`, `closed_no_input`,
`closed_no_demand`, `closed_broke`). These counters stay out of the hash:


```bash
cargo run --release -p timeloop-sim -- --years 30 --snapshots out
```

`cargo test` runs the fast tests only; the long acceptance runs are
`#[ignore]`d and run in CI (`cargo test -- --include-ignored`).

Add `--stats FILE` to write one CSV row per day (money of agents, businesses and in total, prices, wages, cash,
staff, stock, employment, hunger, meals, work hours, money Gini, share of
time spent starving, rent and tenants per house, homeless, mean commute,
founded firms open / founded / closed, money held by firm owners, deaths,
births, newcomers, mean age, people in education and retired, price index,
treasury per association, mean level of workers, people, newcomers who
moved in for work, beds, houses built, comfort goods bought):

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

Every kind of business is a profession (`[career]`), as in Ages of Echo:
an hour of work gives experience in it, level 1 needs 50 and each further
level 6% more; every level adds 2% output and 2% wage, and businesses hire
the most experienced job seeker first. Prices clear daily: each moves by the
ratio of demand (sold plus buyers who found nothing) plus target stock to
what was on offer, at most 5% a day (`market.price_change_percent`); prices
are kept in thousandths, sales use them rounded.

Associations (`[[association]]`) are guilds (covering some goods; members are
the businesses making them) or the town council (covering every good;
members are all people). Once a week each collects dues, pays out its
treasury (a guild to its member businesses, the council into welfare) and
decides its rules anew from formulas. Rules change what actions cost or
allow: `min_price` / `max_price` for sellers, `founding_fee` for founders
(`market.fee` in the founding formulas), `dues_percent` on members' sales,
`sales_tax_percent` on every meal (included in `food_price`). The bundled
bakers' guild sets bread's minimum price to cost plus three quarters, takes 5% dues and
200 per new bakery; the council taxes meals by 5%. The price index is what
the cheapest meal costs, tax included, in percent of bread's starting price.

New goods and building (iteration 5c): recipes (`[[recipe]]`: brewery, woodcutter,
carpenter) are kinds of business nobody runs at the start; founders open
them like any other once their good is short and sells well. Beer and
furniture are comfort goods (`goods.comfort`): people spend their comfort
budget on them, and what finds nothing to buy goes to the town. Someone
with enough cash builds a house (`[building]`) when full houses turn people
away; it takes 30 days, its rent goes to the builder after the profit tax.
Work nobody takes for a week draws newcomers (`[growth]`), but only while
there is a free bed and food to spare. The town stands behind its own
businesses: they never close, one below half its reserve is topped up from
the town's income, and a fully staffed one whose good keeps running short
opens a branch from its own cash. Welfare first tops everyone up to a day of
meals; what the town's income lacks for that is levied on savings above the
safety reserve (poor relief).

To watch and steer the town live, start the engine as a local server and
open the page it prints. The engine runs natively in its own thread for up to
80 years (`--years N` to change); the page shows the map as it is right now,
including founded firms (white corner, labelled with the owner) and houses
agents built (corner; dashed while under construction); labels never overlap; with play/pause, a speed control from 1 game minute per second up to as fast
as possible (Space, arrow keys), a history chart to jump to any point (back
via a checkpoint per year), restart with another seed, and for a clicked agent
how it scores every action. Speed never changes the outcome: the state hash
at a given game minute is the same however you got there.

```bash
cargo run --release -p timeloop-sim -- serve        # http://127.0.0.1:7878
```

With `--world content/data/world.toml` the server reloads that file whenever
a page is (re)loaded after it changed, and starts the town over with it.

The newer pages (so far `/rules`: value chain, needs, decision rules) are a
React + TypeScript app in `web/` (Vite; React Flow with ELK for the value
chain). While working on them, run the dev server next to the engine; edits
show at once:

```bash
npm --prefix web install
npm --prefix web run dev                             # http://localhost:5173, /api goes to :7878
```

`npm --prefix web run build` writes `web/dist`; `serve --web web/dist` then
serves it at `/rules` without Node.

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

The default world is a bread town: field -> mill -> bakery, 100 agents. Money is
a closed loop (never created or destroyed); prices and wages move in fixed daily
steps. `sim/tests/bread_town.rs` checks that ten years stay fed and balanced;
`sim/tests/traits.rs` checks that diligent and easygoing populations work
different hours and end up with different wealth; `sim/tests/city.rs` checks
that vacancy settles, rents stay bounded and commutes get shorter;
`sim/tests/firms.rs` checks over six seeds of 20 years that firms get founded
and survive, the town stays fed and owners hold less than half the money;
`sim/tests/life.rs` runs 80 years and checks deaths, births, newcomers and
the age structure; `sim/tests/careers.rs` checks that workers level up, the
experienced save more, prices change by under 5% a day and hunger stays
below iteration 5; `sim/tests/guilds.rs` checks that the guild's minimum
price visibly raises the price index; `sim/tests/building.rs` checks over
six seeds of 30 years that new kinds of business open, comfort goods sell,
houses get built and the town grows while it stays fed.

`content/data/world.toml` points at its schema, so editors with TOML schema
support (Taplo / Even Better TOML) autocomplete and validate it. After changing
the content structs, regenerate it with `UPDATE_GOLDEN=1 cargo test -p timeloop-content`.

Golden event logs in `sim/tests/golden/` pin the exact behaviour. After an
intended change, review the diff and regenerate them with
`UPDATE_GOLDEN=1 cargo test -p timeloop-sim --test golden`.

The same seed must produce the same hashes on
every platform; CI checks this on Linux (x86, ARM), macOS and Windows.

Core rules: integers only, randomness only through `rng`, iteration only in ID order.
