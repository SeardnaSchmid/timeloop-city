// The value chain as a graph: goods, the buildings that make them, what
// houses and new buildings are built from, and the needs goods cover.
// Pure data; layout and drawing live elsewhere.
import type { Rules } from '../api';

export type NodeKind = 'good' | 'building' | 'sink' | 'need';

export interface ChainNode {
  id: string;
  kind: NodeKind;
  title: string;
  lines: string[];
  /** Colour role: town / recipe for buildings. */
  role?: 'town' | 'recipe';
  /** Hover text. */
  tip?: string;
}

export interface ChainEdge {
  id: string;
  source: string;
  target: string;
  label?: string;
  role: 'town' | 'recipe' | 'need' | 'mat';
}

const rate = (milli: number) => (milli / 1000).toFixed(2).replace(/\.?0+$/, '');

/** Cheapest start price of a good across its makers. */
function startPrice(r: Rules, good: number): number {
  const prices = r.makers.filter((m) => m.output === good).map((m) => m.price);
  return prices.length ? Math.min(...prices) : 0;
}

/** Money and parts of a set of materials at start prices. */
export function materialCost(r: Rules, materials: Record<string, number>) {
  const parts = Object.entries(materials);
  const money = parts.reduce((sum, [name, n]) => sum + n * startPrice(r, r.goods.indexOf(name)), 0);
  return { money, text: parts.map(([name, n]) => `${n} ${name}`).join(' + ') };
}

export function buildGraph(r: Rules): { nodes: ChainNode[]; edges: ChainEdge[] } {
  const nodes: ChainNode[] = [];
  const edges: ChainEdge[] = [];
  const good = (i: number) => `good:${i}`;

  // Cost of opening any building: capital plus materials.
  const F = r.founding;
  const open = F ? materialCost(r, F.materials) : null;
  const openText = F && open ? `≈ ${F.capital + open.money} (${F.capital}${open.text ? ' + ' + open.text : ''})` : null;

  // Order nodes chain by chain (raw good first, then what is made from it),
  // so the layout keeps each chain together.
  const inputOf = r.goods.map((_, g) => r.makers.find((m) => m.output === g && m.input)?.input?.[0] ?? null);
  const order: number[] = [];
  const visit = (g: number) => {
    order.push(g);
    r.goods.forEach((_, k) => inputOf[k] === g && visit(k));
  };
  r.goods.forEach((_, g) => inputOf[g] === null && visit(g));

  const uses = (g: number) => {
    const out: string[] = [];
    if (g === r.food) out.push('food');
    return out;
  };
  for (const g of order) {
    r.makers.forEach((m, k) => {
      if (m.output !== g) return;
      const id = `building:${k}`;
      nodes.push({
        id,
        kind: 'building',
        title: m.name,
        role: m.town ? 'town' : 'recipe',
        lines: [
          `${rate(m.per_hour_milli)}/h per worker · ${m.slots} jobs`,
          `wage ${m.wage} · sells at ${m.price}`,
          ...(openText ? [`${m.town ? 'branch' : 'open'} ${openText}`] : []),
        ],
        tip: m.town ? 'Run by the town from the start' : 'Anyone with the money can found one',
      });
      if (m.input) {
        edges.push({
          id: `${id}:in`,
          source: good(m.input[0]),
          target: id,
          label: m.input[1] !== 1 ? `${m.input[1]}×` : undefined,
          role: m.town ? 'town' : 'recipe',
        });
      }
      edges.push({ id: `${id}:out`, source: id, target: good(g), role: m.town ? 'town' : 'recipe' });
    });
    nodes.push({ id: good(g), kind: 'good', title: r.goods[g], lines: uses(g) });
  }

  // Houses and new buildings, and what they are built from.
  const B = r.building;
  if (B) {
    const c = materialCost(r, B.materials);
    nodes.push({
      id: 'sink:house',
      kind: 'sink',
      title: 'house',
      lines: [`${B.beds} beds · rent ${B.rent}`, `≈ ${B.cost + c.money} (${B.cost} + materials)`, `${B.construction_days} days to build`],
    });
    for (const [name, n] of Object.entries(B.materials))
      edges.push({ id: `house:${name}`, source: good(r.goods.indexOf(name)), target: 'sink:house', label: `${n}×`, role: 'mat' });
  }
  if (F && open) {
    nodes.push({
      id: 'sink:building',
      kind: 'sink',
      title: 'new building',
      lines: [`any of the above`, `${F.capital} capital + materials`, `at most ${F.max_firms} firms`],
    });
    for (const [name, n] of Object.entries(F.materials))
      edges.push({ id: `open:${name}`, source: good(r.goods.indexOf(name)), target: 'sink:building', label: `${n}×`, role: 'mat' });
  }

  // Needs.
  const N = r.needs;
  nodes.push({ id: 'need:hunger', kind: 'need', title: 'hunger · tier 1', lines: [`+${N.hunger.per_hour}/h`] });
  edges.push({ id: 'eat', source: good(r.food), target: 'need:hunger', label: `−${r.meal_relief}`, role: 'need' });
  nodes.push({ id: 'need:fatigue', kind: 'need', title: 'fatigue · tier 1', lines: [`+${N.fatigue.per_hour}/h, sleep in a bed`] });
  if (B) edges.push({ id: 'sleep', source: 'sink:house', target: 'need:fatigue', label: 'beds', role: 'need' });
  nodes.push({
    id: 'need:safety',
    kind: 'need',
    title: 'safety · tier 2',
    lines: [`savings for ${N.safety.reserve_days} days of meals`],
  });
  nodes.push({ id: 'need:comfort', kind: 'need', title: 'comfort · tier 3', lines: [`+${N.comfort.per_hour}/h, buys from ${N.comfort.buy_at}`] });
  for (const [g, relief] of r.comfort)
    edges.push({ id: `comfort:${g}`, source: good(g), target: 'need:comfort', label: `−${relief}`, role: 'need' });

  return { nodes, edges };
}
