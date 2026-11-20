import type { Speed } from './types';

/** Game minutes per real second; the last stop is "as fast as possible". */
export const SPEEDS: Speed[] = [1, 5, 15, 60, 240, 1440, 10080, 43800, 131400, 525600, 'max'];

export const speedName = (v: Speed) =>
  v === 'max' ? 'max'
  : v < 60 ? `${v} min/s`
  : v < 1440 ? `${v / 60} h/s`
  : v < 10080 ? `${v / 1440} day/s`
  : v < 43800 ? `${v / 10080} week/s`
  : v < 525600 ? `${Math.round(v / 43800)} month/s`
  : '1 year/s';

export const rate = (m: number) =>
  m < 60 ? `${Math.round(m)} min/s`
  : m < 1440 ? `${(m / 60).toFixed(1)} h/s`
  : m < 43800 ? `${(m / 1440).toFixed(1)} days/s`
  : `${(m / 525600).toFixed(2)} years/s`;

export function fmtTime(t: number) {
  const d = Math.floor(t / 1440), m = t % 1440;
  const hh = String(Math.floor(m / 60)).padStart(2, '0'), mm = String(m % 60).padStart(2, '0');
  return `year ${Math.floor(d / 365)} · day ${d % 365} · ${hh}:${mm}`;
}

/** A stable colour per good, so any number of goods gets told apart. */
export function goodColor(good: string) {
  let h = 0;
  for (const c of good) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return `hsl(${h % 360} 55% 45%)`;
}

export const css = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();
