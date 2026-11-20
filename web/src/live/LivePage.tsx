// /: the town running live, with controls, map, history and details.
import { useEffect, useState } from 'react';
import { getJson } from '../api';
import { fmtTime, rate, SPEEDS, speedName } from './format';
import { HistoryChart } from './HistoryChart';
import { MapCanvas } from './MapCanvas';
import { ActionLegend, SelectionPanel, TownPanel } from './Panels';
import type { Explain, Selection } from './types';
import { useEngine } from './useEngine';
import './live.css';

export function LivePage() {
  const { layout: L, state: S, prev, history, error, control } = useEngine();
  const [selected, setSelected] = useState<Selection>(null);
  const [explain, setExplain] = useState<Explain | null>(null);
  const [showLand, setShowLand] = useState(false);
  const [seed, setSeed] = useState('');
  const speedIndex = S ? Math.max(0, SPEEDS.indexOf(S.speed)) : 3;

  // How the selected agent scores its actions, refreshed while it runs.
  useEffect(() => {
    setExplain(null);
    if (selected?.type !== 'agent') return;
    const load = () => getJson<Explain>(`/api/explain?agent=${selected.id}`).then(setExplain, () => {});
    load();
    const t = setInterval(load, 700);
    return () => clearInterval(t);
  }, [selected]);

  // Space: play/pause. ←/→: slower/faster.
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (!S || (e.target as HTMLElement).tagName === 'INPUT') return;
      if (e.code === 'Space') { e.preventDefault(); control({ play: S.playing ? 0 : 1 }); }
      if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
        e.preventDefault();
        const i = Math.max(0, Math.min(SPEEDS.length - 1, speedIndex + (e.key === 'ArrowRight' ? 1 : -1)));
        control({ speed: SPEEDS[i] });
      }
    };
    document.addEventListener('keydown', key);
    return () => document.removeEventListener('keydown', key);
  }, [S, speedIndex, control]);

  if (!L) return <div className="page">{error ? <span className="error">{error}</span> : 'connecting…'}</div>;
  const status = error ? <span className="error">{error}</span>
    : !S ? 'connecting…'
    : S.seeking !== null ? `jumping to ${fmtTime(S.seeking)}…`
    : S.playing || S.effective > 1 ? `running at ${rate(S.effective)}`
    : S.world.now >= S.end ? 'end of run' : 'paused';

  return (
    <main className="live">
      <header>
        <h1>Timeloop City</h1>
        <span className="clock">{S ? fmtTime(S.world.now) : '…'}</span>
        <button className="primary" title="Space" onClick={() => S && control({ play: S.playing ? 0 : 1 })}>
          {S?.playing ? 'Pause' : 'Play'}
        </button>
        <span className="speed">
          <input type="range" min={0} max={SPEEDS.length - 1} value={speedIndex} title="← / →" aria-label="Speed"
            onChange={(e) => control({ speed: SPEEDS[+e.target.value] })} />
          <output>{speedName(SPEEDS[speedIndex])}</output>
        </span>
        <span className="muted">{status}</span>
        <span style={{ flex: 1 }} />
        <label className="muted">
          seed <input type="number" min={0} value={seed || String(S?.world.seed ?? '')} onChange={(e) => setSeed(e.target.value)} />
        </label>
        <button onClick={() => { setSelected(null); control({ restart: 1, seed: seed || S?.world.seed || 0, play: 0 }); }}>Restart</button>
        <a href="/rules" target="_blank" rel="noopener">Rules</a>
        <label className="muted">
          <input type="checkbox" checked={showLand} onChange={(e) => setShowLand(e.target.checked)} /> land price
        </label>
      </header>
      <div className="map card">{S && <MapCanvas layout={L} state={S} prev={prev} showLand={showLand} selected={selected} onSelect={setSelected} />}</div>
      <aside>
        <div className="card">
          <h2>Selection</h2>
          {S && <SelectionPanel L={L} S={S} selected={selected} explain={explain} />}
        </div>
        <div className="card">
          <h2>Town</h2>
          {S && <TownPanel S={S} />}
        </div>
        <div className="card">
          <h2>Agents</h2>
          <ActionLegend L={L} />
        </div>
      </aside>
      <div className="card wide">
        <h2>History · click to jump</h2>
        <HistoryChart layout={L} rows={history} population={S?.world.population ?? 1} now={S?.world.now ?? 0}
          onSeek={(m) => control({ seek: m })} />
      </div>
    </main>
  );
}
