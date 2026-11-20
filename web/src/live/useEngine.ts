// Connection to the live engine: layout once, state polled every frame,
// history appended in the background, and controls.
import { useCallback, useEffect, useRef, useState } from 'react';
import { getJson } from '../api';
import type { EngineState, HistoryChunk, Layout } from './types';

export function useEngine() {
  const [layout, setLayout] = useState<Layout | null>(null);
  const [state, setState] = useState<EngineState | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** The state before the current one, for gliding agents. */
  const prev = useRef<{ state: EngineState | null; at: number }>({ state: null, at: 0 });
  const [history, setHistory] = useState<{ epoch: number; rows: number[] }>({ epoch: -1, rows: [] });

  const accept = useCallback((st: EngineState) => {
    setState((old) => {
      prev.current = { state: old && old.world.epoch === st.world.epoch ? old : null, at: performance.now() };
      return st;
    });
  }, []);

  useEffect(() => {
    getJson<Layout>('/api/layout').then(setLayout, (e: Error) => setError(e.message));
  }, []);

  // One state request at a time, as often as the screen redraws.
  useEffect(() => {
    if (!layout) return;
    let alive = true, busy = false, raf = 0;
    const tick = () => {
      if (!alive) return;
      if (!busy) {
        busy = true;
        getJson<EngineState>('/api/state')
          .then((st) => { accept(st); setError(null); })
          .catch(() => setError('engine not reachable – is `sim serve` running?'))
          .finally(() => { busy = false; });
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => { alive = false; cancelAnimationFrame(raf); };
  }, [layout, accept]);

  // History: every half second, fetch the days not seen yet; start over on a restart.
  const latest = useRef({ epoch: -1, days: 0, fields: 1, history });
  latest.current = { epoch: state?.world.epoch ?? -1, days: state?.world.days ?? 0, fields: layout?.historyFields.length ?? 1, history };
  useEffect(() => {
    let busy = false;
    const t = setInterval(() => {
      const { epoch, days, fields, history } = latest.current;
      const have = history.epoch === epoch ? history.rows.length / fields : 0;
      if (busy || epoch < 0 || (history.epoch === epoch && have >= days)) return;
      busy = true;
      getJson<HistoryChunk>(`/api/history?from=${have}`)
        .then((h) =>
          setHistory((old) => {
            const rows = old.epoch === h.epoch ? old.rows : [];
            return h.from === rows.length / fields ? { epoch: h.epoch, rows: rows.concat(h.rows) } : old;
          }),
        )
        .finally(() => { busy = false; });
    }, 500);
    return () => clearInterval(t);
  }, []);
  const epoch = state?.world.epoch ?? -1;

  const control = useCallback(
    async (params: Record<string, string | number>) => {
      const q = new URLSearchParams(Object.entries(params).map(([k, v]) => [k, String(v)]));
      const res = await fetch(`/api/control?${q}`, { method: 'POST' });
      if (res.ok) accept(await res.json());
    },
    [accept],
  );

  return { layout, state, prev, history: history.epoch === epoch ? history.rows : [], error, control };
}
