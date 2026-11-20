// Side panels: the selection (with how an agent scores its actions), the
// town in numbers, and the action legend.
import type { ReactNode } from 'react';
import { agentAt, type EngineState, type Explain, type Layout, type Selection } from './types';

const Table = ({ rows }: { rows: [ReactNode, ReactNode][] }) => (
  <table className="kv">
    <tbody>
      {rows.map(([k, v], i) => (
        <tr key={i}>
          <td>{k}</td>
          <td className="num">{v}</td>
        </tr>
      ))}
    </tbody>
  </table>
);

export function SelectionPanel({ L, S, selected, explain }: { L: Layout; S: EngineState; selected: Selection; explain: Explain | null }) {
  const w = S.world;
  if (!selected) return <div className="muted">Click an agent or a building.</div>;
  if (selected.type === 'agent') {
    const k = selected.id, a = agentAt(S, k);
    const stage = ['in education', 'working age', 'retired'][w.stages[k]];
    const job = a.job >= 0 ? w.businesses[a.job] : null;
    return (
      <>
        <h3>Agent a{k}</h3>
        <Table
          rows={[
            ['age', `${w.ages[k]} (${stage})`],
            ['parent', w.parents[k] >= 0 ? 'a' + w.parents[k] : '–'],
            ['generation in this slot', w.generations[k] + 1],
            ['level in job', w.levels[k] >= 0 ? w.levels[k] : '–'],
            ['doing', L.actions[a.act]],
            ['hunger', a.hunger],
            ['money', a.money],
            ['home', a.home >= 0 ? w.houses[a.home].name : 'homeless'],
            ['job', job ? job.name + (job.owner === k ? ' (CEO)' : '') : 'unemployed'],
            ...L.traits.map((t, n): [string, number] => [t, w.traits[k][n]]),
          ]}
        />
        {explain && (
          <>
            <h3 style={{ marginTop: 12 }}>Would pick now: {explain.winner}</h3>
            <table className="kv">
              <tbody>
                {explain.candidates.map((c) => (
                  <tr key={c.kind} className={c.kind === explain.winner ? 'win' : ''}>
                    <td>
                      {c.kind}
                      <div className="formula">{c.formula}</div>
                    </td>
                    <td className="num">
                      {c.score}
                      <div className="formula">{c.allowed}</div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </>
        )}
      </>
    );
  }
  if (selected.type === 'house') {
    const h = w.houses[selected.id];
    if (!h) return null;
    return (
      <>
        <h3>House · {h.name}</h3>
        <Table
          rows={[
            ['built by', h.owner >= 0 ? 'a' + h.owner : 'town'],
            ['state', h.building ? 'under construction' : 'lived in'],
            ['rent / day', w.rent[selected.id]],
            ['tenants', `${w.tenants[selected.id]} / ${h.beds}`],
            ['parcel', `(${h.x}, ${h.y})`],
            ['land price', w.land[h.y * L.w + h.x]],
          ]}
        />
      </>
    );
  }
  const b = w.businesses[selected.id];
  if (!b) return null;
  return (
    <>
      <h3>
        {b.owner >= 0 ? 'Firm' : 'Town business'} · {b.name}
      </h3>
      <Table
        rows={[
          ['open', b.open ? `since day ${b.since}` : 'closed'],
          ['owner', b.owner >= 0 ? 'a' + b.owner : 'town'],
          ['makes', b.good],
          ['price', b.price],
          ['wage / h', b.wage],
          ['cash', b.cash],
          ['workers', `${b.workers} / ${b.slots}`],
          ['parcel', `(${b.x}, ${b.y})`],
        ]}
      />
    </>
  );
}

export function TownPanel({ S }: { S: EngineState }) {
  const w = S.world;
  const hungry = Array.from({ length: w.agents.length / 7 }, (_, k) => agentAt(S, k).hunger).filter((h) => h >= 800).length;
  const goods = [...new Set(w.businesses.map((b) => b.good))];
  const cheapest = (g: string) => {
    const p = w.businesses.filter((b) => b.open && b.good === g).map((b) => b.price);
    return p.length ? Math.min(...p) : '–';
  };
  return (
    <Table
      rows={[
        ['people (moved in for work)', `${w.population} (${w.immigrants})`],
        ['employed', `${w.employed} / ${w.population}`],
        ['homeless', w.homeless],
        ['hungry (≥800)', hungry],
        ['founded firms open', w.firms],
        ['houses built / beds', `${w.housesBuilt} / ${w.beds}`],
        ['comfort goods bought', w.comforts],
        ['money in the world', w.money],
        ['mean age', w.meanAge],
        ['in education / retired', `${w.inEducation} / ${w.retired}`],
        ['deaths / born / moved in', `${w.deaths} / ${w.births} / ${w.newcomers}`],
        ['price index (meal incl. tax)', w.priceIndex],
        ['mean level of workers', w.meanLevel],
        ...w.associations.map((a): [string, string] => [
          `${a.name} (${a.members}) · cash ${a.cash}`,
          a.rules.map(([r, v]) => `${r} ${v}`).join(', ') || '–',
        ]),
        ...goods.map((g): [string, string | number] => [`${g} price (lowest)`, cheapest(g)]),
      ]}
    />
  );
}

export function ActionLegend({ L }: { L: Layout }) {
  return (
    <div className="legend">
      {L.actions
        .filter((a) => a !== 'idle')
        .map((a) => (
          <span key={a}>
            <span className="sw" style={{ background: `var(--${a})` }} />
            {a}
          </span>
        ))}
    </div>
  );
}
