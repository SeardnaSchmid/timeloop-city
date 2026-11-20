// /rules: the running world's value chain, needs and decision rules.
import { useEffect, useState, type ReactNode } from 'react';
import { fetchRules, type Rules } from '../api';
import { Formula } from './Formula';
import { materialCost } from './graph';
import { ValueChain } from './ValueChain';

const Card = ({ title, children }: { title: string; children: ReactNode }) => (
  <div className="card">
    <h2>{title}</h2>
    {children}
  </div>
);

const Row = ({ k, v }: { k: string; v: ReactNode }) => (
  <tr>
    <td>{k}</td>
    <td className="num">{v}</td>
  </tr>
);

export function RulesPage() {
  const [rules, setRules] = useState<Rules | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    fetchRules().then(setRules, (e: Error) => setError(e.message));
  }, []);
  if (error) return <div className="page error">{error}</div>;
  if (!rules) return <div className="page muted">loading…</div>;
  const r = rules;
  const N = r.needs;
  const F = r.founding;
  const B = r.building;
  return (
    <div className="page">
      <div className="head">
        <h1>Timeloop City · Rules</h1>
        <span className="muted">
          {r.agents} agents · {r.goods.length} goods · {r.makers.length} kinds of building · {r.actions.length} actions
        </span>
      </div>

      <Card title="Value chain: what is made where, from what, and what for">
        <ValueChain rules={r} />
        <p className="muted" style={{ marginBottom: 0 }}>
          Round: goods. Square: buildings that make them (<span style={{ color: 'var(--town)' }}>solid blue</span>: the town runs one
          from the start, <span style={{ color: 'var(--recipe)' }}>dashed violet</span>: anyone with the money can open one), with output,
          jobs and what a new one costs at start prices. Grey: what houses and new buildings are built from. Orange: the needs a good
          covers. Drag to pan, scroll to zoom.
        </p>
      </Card>

      <div className="grid">
        <Card title="Needs (pyramid)">
          <table>
            <thead>
              <tr><th>need</th><th>tier</th><th>rule</th></tr>
            </thead>
            <tbody>
              <tr><td>hunger</td><td>1</td><td>+{N.hunger.per_hour}/h; a meal −{N.hunger.meal_relief} ({N.hunger.meal_minutes} min); from {N.hunger.critical} drops everything</td></tr>
              <tr><td>fatigue</td><td>1</td><td>+{N.fatigue.per_hour}/h; sleep −{N.fatigue.recovery_per_hour}/h, at most {N.fatigue.max_sleep_minutes} min</td></tr>
              <tr><td>safety</td><td>2</td><td>1000 without savings, 0 with {N.safety.reserve_days} days × {N.safety.meals_per_day} meals saved</td></tr>
              <tr><td>comfort</td><td>3</td><td>+{N.comfort.per_hour}/h; from {N.comfort.buy_at} buys with up to {N.comfort.spend_permille / 10}% of spare savings</td></tr>
            </tbody>
          </table>
        </Card>

        <Card title="Buildings that make goods">
          <table>
            <thead>
              <tr><th>building</th><th>makes</th><th>from</th><th className="num">/worker·h</th><th className="num">jobs</th><th className="num">price</th><th className="num">wage</th></tr>
            </thead>
            <tbody>
              {r.makers.map((m) => (
                <tr key={m.name}>
                  <td style={{ color: m.town ? 'var(--town)' : 'var(--recipe)' }}>{m.name}</td>
                  <td>{r.goods[m.output]}</td>
                  <td>{m.input ? `${m.input[1]} ${r.goods[m.input[0]]}` : '—'}</td>
                  <td className="num">{m.per_hour_milli / 1000}</td>
                  <td className="num">{m.slots}</td>
                  <td className="num">{m.price}</td>
                  <td className="num">{m.wage}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      </div>

      <div className="grid">
        <Card title="Decisions: every wake-up, the allowed action with the highest score wins">
          {r.actions.map((a) => (
            <div key={a.kind} style={{ marginBottom: 8 }}>
              <b>{a.kind}</b>
              {a.when && <Formula label="when" src={a.when} />}
              <Formula label="score" src={a.score} />
            </div>
          ))}
        </Card>
      </div>

      <div className="grid">
        <Card title="Opening a building">
          {F ? (
            <>
              <div className="muted">
                {F.capital} capital + {materialCost(r, F.materials).text || 'no materials'} · profit tax {F.profit_tax_percent}% · at most {F.max_firms} firms
              </div>
              <Formula label="when" src={F.when} />
              <Formula label="score" src={F.score} />
              <Formula label="close" src={F.close} />
            </>
          ) : (
            <span className="muted">off: only the town runs businesses</span>
          )}
        </Card>
        <Card title="Building houses">
          {B ? (
            <>
              <div className="muted">
                {B.cost} + {materialCost(r, B.materials).text || 'no materials'} · {B.beds} beds · rent {B.rent} · {B.construction_days} days ·
                at most {B.max_houses} by agents
              </div>
              <Formula label="when" src={B.when} />
              <Formula label="score" src={B.score} />
              <div className="muted" style={{ fontSize: 12, marginTop: 6 }}>If nobody builds and more are homeless than beds are free, the town builds.</div>
            </>
          ) : (
            <span className="muted">off</span>
          )}
        </Card>
        <Card title="Choosing a home">
          <Formula label="when" src={r.housing.when} />
          <Formula label="score" src={r.housing.score} />
          <Formula label="homeless" src={r.housing.homeless_score} />
        </Card>
        <Card title="Market and growth">
          <table>
            <tbody>
              <Row k="price change per day" v={`≤ ${r.market.price_change_percent}%`} />
              <Row k="lowest price" v={r.market.price_min} />
              <Row k="wage step / minimum" v={`${r.market.wage_step} / ${r.market.wage_min}`} />
              <Row k="firm cash reserve" v={`${r.market.reserve_days} days of payroll`} />
              <Row k="start money per agent" v={r.start_money} />
              <Row k="growth" v={r.growth ? `up to ${r.growth.max_agents}, ${r.growth.arrivals_per_day}/day` : 'off'} />
              <Row k="traits" v={r.traits.join(', ')} />
            </tbody>
          </table>
        </Card>
      </div>
    </div>
  );
}
