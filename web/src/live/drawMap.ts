// Draws the town on a canvas: land price, buildings with labels that never
// overlap, and agents coloured by what they do.
import { css, goodColor } from './format';
import { agentAt, people, type EngineState, type Layout, type Selection } from './types';

export interface AgentDot { cx: number; cy: number }

export function drawMap(
  ctx: CanvasRenderingContext2D,
  L: Layout,
  S: EngineState,
  prev: EngineState | null,
  glide: number,
  cell: number,
  showLand: boolean,
  selected: Selection,
): AgentDot[] {
  const W = L.w * cell, H = L.h * cell, w = S.world;
  ctx.clearRect(0, 0, W, H + cell);
  if (showLand) {
    const max = Math.max(...w.land), min = Math.min(...w.land), rgb = css('--land');
    for (let y = 0; y < L.h; y++)
      for (let x = 0; x < L.w; x++) {
        ctx.fillStyle = `rgba(${rgb}, ${0.05 + ((w.land[y * L.w + x] - min) / Math.max(1, max - min)) * 0.35})`;
        ctx.fillRect(x * cell, y * cell, cell, cell);
      }
  }
  ctx.strokeStyle = css('--grid');
  ctx.lineWidth = 1;
  ctx.beginPath();
  for (let x = 0; x <= L.w; x++) { ctx.moveTo(x * cell + 0.5, 0); ctx.lineTo(x * cell + 0.5, H); }
  for (let y = 0; y <= L.h; y++) { ctx.moveTo(0, y * cell + 0.5); ctx.lineTo(W, y * cell + 0.5); }
  ctx.stroke();

  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  // Labels skip any spot an earlier one took; the selected one goes first.
  const taken: number[][] = [];
  const fs = Math.max(9, cell * 0.32);
  const label = (txt: string, x: number, y: number) => {
    ctx.font = `${fs}px system-ui`;
    const half = ctx.measureText(txt).width / 2 + 2, h = fs + 2;
    const cx = Math.min(Math.max((x + 0.5) * cell, half), W - half), cy = (y + 1) * cell + cell * 0.3;
    const box = [cx - half, cy - h / 2, cx + half, cy + h / 2];
    if (taken.some((t) => box[0] < t[2] && t[0] < box[2] && box[1] < t[3] && t[1] < box[3])) return;
    taken.push(box);
    ctx.fillStyle = css('--ink');
    ctx.fillText(txt, cx, cy);
  };
  const bizLabel = (k: number) => {
    const b = w.businesses[k];
    return `${b.owner >= 0 ? 'a' + b.owner : b.name} ${b.workers}/${b.slots} · ${b.price}`;
  };
  const houseLabel = (k: number) => {
    const h = w.houses[k];
    return h.building ? `${h.name} (building)` : `${h.name} ${w.tenants[k]}/${h.beds} · rent ${w.rent[k]}`;
  };
  if (selected?.type === 'business' && w.businesses[selected.id]) {
    const b = w.businesses[selected.id];
    label(bizLabel(selected.id), b.x, b.y);
  }
  if (selected?.type === 'house' && w.houses[selected.id]) {
    const h = w.houses[selected.id];
    label(houseLabel(selected.id), h.x, h.y);
  }
  w.businesses.forEach((b, k) => {
    if (!b.open) return;
    ctx.fillStyle = goodColor(b.good);
    ctx.fillRect(b.x * cell + 2, b.y * cell + 2, cell - 4, cell - 4);
    // Founded firms: a white corner marks an owner.
    if (b.owner >= 0) {
      ctx.fillStyle = '#fff';
      ctx.beginPath();
      ctx.moveTo(b.x * cell + 2, b.y * cell + 2);
      ctx.lineTo(b.x * cell + 2 + cell * 0.35, b.y * cell + 2);
      ctx.lineTo(b.x * cell + 2, b.y * cell + 2 + cell * 0.35);
      ctx.fill();
    }
    ctx.fillStyle = '#fff';
    ctx.font = `bold ${cell * 0.45}px system-ui`;
    ctx.fillText(b.name[0].toUpperCase(), (b.x + 0.5) * cell, (b.y + 0.5) * cell);
    label(bizLabel(k), b.x, b.y);
  });
  w.houses.forEach((h, k) => {
    const x = h.x * cell + 2, y = h.y * cell + 2, s = cell - 4;
    ctx.fillStyle = css('--house');
    ctx.globalAlpha = h.building ? 0.05 : 0.15;
    ctx.fillRect(x, y, s, s);
    ctx.globalAlpha = 1;
    ctx.strokeStyle = css('--house');
    ctx.lineWidth = 2;
    // Under construction: dashed. Built by an agent: a filled corner.
    if (h.building) ctx.setLineDash([3, 3]);
    ctx.strokeRect(x + 1, y + 1, s - 2, s - 2);
    ctx.setLineDash([]);
    ctx.lineWidth = 1;
    if (h.owner >= 0) {
      ctx.fillStyle = css('--house');
      ctx.beginPath();
      ctx.moveTo(x, y);
      ctx.lineTo(x + cell * 0.35, y);
      ctx.lineTo(x, y + cell * 0.35);
      ctx.fill();
    }
    label(houseLabel(k), h.x, h.y);
  });
  if (selected?.type === 'agent' && selected.id < people(S)) {
    const a = agentAt(S, selected.id);
    for (const p of [a.home >= 0 ? w.houses[a.home] : null, a.job >= 0 ? w.businesses[a.job] : null]) {
      if (!p) continue;
      ctx.strokeStyle = css('--sel');
      ctx.lineWidth = 2;
      ctx.strokeRect(p.x * cell + 1, p.y * cell + 1, cell - 2, cell - 2);
      ctx.lineWidth = 1;
    }
  }

  // Agents glide from the previous state; several on one parcel spread out.
  const pos: { x: number; y: number; act: number }[] = [];
  const groups = new Map<string, number[]>();
  for (let k = 0; k < people(S); k++) {
    const a = agentAt(S, k), b = prev && k < people(prev) ? agentAt(prev, k) : a;
    const x = b.x + (a.x - b.x) * glide, y = b.y + (a.y - b.y) * glide;
    const key = `${Math.round(x)},${Math.round(y)}`;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key)!.push(k);
    pos[k] = { x, y, act: a.act };
  }
  const dots: AgentDot[] = [];
  const r = Math.max(2.5, cell * 0.11);
  for (const ks of groups.values()) {
    const n = ks.length, cols = Math.ceil(Math.sqrt(n));
    ks.forEach((k, idx) => {
      const p = pos[k];
      const ox = n === 1 ? 0 : ((idx % cols) - (cols - 1) / 2) * r * 2.2;
      const oy = n === 1 ? 0 : (Math.floor(idx / cols) - (Math.ceil(n / cols) - 1) / 2) * r * 2.2;
      const cx = (p.x + 0.5) * cell + ox, cy = (p.y + 0.5) * cell + oy;
      dots[k] = { cx, cy };
      ctx.beginPath();
      ctx.arc(cx, cy, r, 0, Math.PI * 2);
      ctx.fillStyle = css('--' + L.actions[p.act]);
      ctx.fill();
      ctx.strokeStyle = css('--panel');
      ctx.stroke();
      if (selected?.type === 'agent' && selected.id === k) {
        ctx.beginPath();
        ctx.arc(cx, cy, r + 2.5, 0, Math.PI * 2);
        ctx.strokeStyle = css('--sel');
        ctx.lineWidth = 2;
        ctx.stroke();
        ctx.lineWidth = 1;
      }
    });
  }
  return dots;
}
