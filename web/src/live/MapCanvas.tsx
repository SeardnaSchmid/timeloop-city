// The town map: redrawn every frame from the latest state; click to select.
import { useEffect, useRef, type MutableRefObject } from 'react';
import { drawMap, type AgentDot } from './drawMap';
import type { EngineState, Layout, Selection } from './types';

interface Props {
  layout: Layout;
  state: EngineState;
  prev: MutableRefObject<{ state: EngineState | null; at: number }>;
  showLand: boolean;
  selected: Selection;
  onSelect: (s: Selection) => void;
}

export function MapCanvas({ layout: L, state, prev, showLand, selected, onSelect }: Props) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const cell = useRef(30);
  const dots = useRef<AgentDot[]>([]);
  const latest = useRef({ state, showLand, selected });
  latest.current = { state, showLand, selected };

  // Size to the container.
  useEffect(() => {
    const c = canvas.current!;
    const fit = () => {
      const w = c.parentElement!.clientWidth - 16;
      cell.current = Math.max(12, Math.floor(w / L.w));
      const dpr = window.devicePixelRatio || 1, cw = L.w * cell.current, ch = (L.h + 1) * cell.current;
      c.width = cw * dpr;
      c.height = ch * dpr;
      c.style.width = `${cw}px`;
      c.style.height = `${ch}px`;
      c.getContext('2d')!.setTransform(dpr, 0, 0, dpr, 0, 0);
    };
    fit();
    const ro = new ResizeObserver(fit);
    ro.observe(c.parentElement!);
    return () => ro.disconnect();
  }, [L]);

  // Draw every frame, so agents glide between states.
  useEffect(() => {
    let raf = 0;
    const frame = () => {
      const { state: S, showLand, selected } = latest.current;
      const p = prev.current;
      const slow = p.state && S.speed !== 'max' && S.speed <= 240;
      const glide = slow ? Math.min(1, (performance.now() - p.at) / 120) : 1;
      dots.current = drawMap(canvas.current!.getContext('2d')!, L, S, p.state, glide, cell.current, showLand, selected);
      raf = requestAnimationFrame(frame);
    };
    raf = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(raf);
  }, [L, prev]);

  const click = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const r = e.currentTarget.getBoundingClientRect(), mx = e.clientX - r.left, my = e.clientY - r.top;
    let best = -1, bd = (cell.current * 0.3) ** 2;
    dots.current.forEach((p, k) => {
      const d = (p.cx - mx) ** 2 + (p.cy - my) ** 2;
      if (d < bd) { bd = d; best = k; }
    });
    if (best >= 0) return onSelect({ type: 'agent', id: best });
    const x = Math.floor(mx / cell.current), y = Math.floor(my / cell.current), w = latest.current.state.world;
    const h = w.houses.findIndex((h) => h.x === x && h.y === y);
    const b = w.businesses.findIndex((b) => b.open && b.x === x && b.y === y);
    onSelect(h >= 0 ? { type: 'house', id: h } : b >= 0 ? { type: 'business', id: b } : null);
  };

  return <canvas ref={canvas} onClick={click} style={{ display: 'block', cursor: 'crosshair' }} />;
}
