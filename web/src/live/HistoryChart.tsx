// History over the whole run (ECharts); click to jump there.
import { useEffect, useRef } from 'react';
import * as echarts from 'echarts';
import { css } from './format';
import type { Layout } from './types';

const COLORS = ['--s1', '--s2', '--s3', '--s4', '--s5', '--s6', '--s7'];

interface Props {
  layout: Layout;
  rows: number[];
  population: number;
  now: number;
  onSeek: (minute: number) => void;
}

export function HistoryChart({ layout: L, rows, population, now, onSeek }: Props) {
  const el = useRef<HTMLDivElement>(null);
  const chart = useRef<echarts.ECharts | null>(null);
  const seek = useRef(onSeek);
  seek.current = onSeek;
  const endDays = L.end / 1440;

  useEffect(() => {
    const c = echarts.init(el.current!);
    chart.current = c;
    c.getZr().on('click', (e) => {
      const [day] = c.convertFromPixel({ gridIndex: 0 }, [e.offsetX, e.offsetY]);
      const d = Math.round(Math.max(0, Math.min(endDays, day)));
      seek.current(Math.min(d * 1440 + 8 * 60, L.end));
    });
    const ro = new ResizeObserver(() => c.resize());
    ro.observe(el.current!);
    return () => { ro.disconnect(); c.dispose(); };
  }, [L, endDays]);

  useEffect(() => {
    const f = L.historyFields.length, n = rows.length / f;
    // Each line on its own scale (share of its max), the tooltip shows the value.
    const scale = (i: number) => {
      if (i <= 1) return Math.max(1, population);
      if (i === 3 || i === 4) return 1000;
      let m = 1;
      for (let d = 0; d < n; d++) m = Math.max(m, rows[d * f + i]);
      return m;
    };
    const series = L.historyFields.map((name, i) => {
      const s = scale(i), data: [number, number, number][] = [];
      for (let d = 0; d < n; d++) data.push([d, (rows[d * f + i] / s) * 100, rows[d * f + i]]);
      return {
        name, type: 'line' as const, data, showSymbol: false, sampling: 'lttb' as const,
        lineStyle: { width: 1.5 }, color: css(COLORS[i % COLORS.length]),
      };
    });
    const years = endDays / 365, every = years > 40 ? 10 : years > 10 ? 5 : 1;
    chart.current?.setOption({
      animation: false,
      grid: { left: 8, right: 8, top: 30, bottom: 22 },
      legend: { top: 0, textStyle: { color: css('--muted') }, itemWidth: 14 },
      tooltip: {
        trigger: 'axis',
        formatter: (ps: { seriesName: string; value: [number, number, number]; color: string }[]) =>
          `day ${ps[0]?.value[0]} (year ${Math.floor((ps[0]?.value[0] ?? 0) / 365)})<br>` +
          ps.map((p) => `<span style="color:${p.color}">●</span> ${p.seriesName}: ${p.value[2]}`).join('<br>'),
      },
      xAxis: {
        type: 'value', min: 0, max: endDays, interval: every * 365,
        axisLabel: { color: css('--muted'), formatter: (d: number) => `year ${Math.round(d / 365)}` },
        splitLine: { lineStyle: { color: css('--grid') } },
      },
      yAxis: { type: 'value', min: 0, max: 100, show: false },
      series,
    }, { replaceMerge: ['series'] });
  }, [L, rows, population, endDays]);

  // The current day, as a line; only redrawn when the day changes.
  const day = Math.floor(now / 1440);
  useEffect(() => {
    chart.current?.setOption({
      series: [{ markLine: { symbol: 'none', silent: true, animation: false, label: { show: false },
        lineStyle: { color: css('--sel'), width: 2, type: 'solid' }, data: [{ xAxis: day }] } }],
    });
  }, [day, rows]);

  return <div ref={el} style={{ width: '100%', height: 190, cursor: 'pointer' }} />;
}
