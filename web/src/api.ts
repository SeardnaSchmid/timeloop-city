// Types and fetchers for the simulation's JSON API (`sim serve`).

export type GoodId = number;

export interface Maker {
  name: string;
  /** Run by the town from the start; otherwise a recipe founders can open. */
  town: boolean;
  output: GoodId;
  /** [good, units per output unit] */
  input: [GoodId, number] | null;
  per_hour_milli: number;
  slots: number;
  price: number;
  wage: number;
  cash: number;
}

export interface Rule {
  when: string;
  score: string;
}

export interface Rules {
  goods: string[];
  food: GoodId;
  meal_relief: number;
  /** [good, relief] */
  comfort: [GoodId, number][];
  makers: Maker[];
  actions: (Rule & { kind: string })[];
  needs: {
    hunger: { per_hour: number; critical: number; meal_relief: number; meal_minutes: number };
    fatigue: { per_hour: number; recovery_per_hour: number; max_sleep_minutes: number };
    safety: { reserve_days: number; meals_per_day: number };
    comfort: { per_hour: number; buy_at: number; spend_permille: number };
  };
  housing: Rule & { homeless_score: string };
  founding:
    | (Rule & { close: string; capital: number; profit_tax_percent: number; max_firms: number; materials: Record<string, number> })
    | null;
  building:
    | (Rule & {
        cost: number;
        beds: number;
        rent: number;
        construction_days: number;
        max_houses: number;
        materials: Record<string, number>;
      })
    | null;
  market: { price_change_percent: number; price_min: number; wage_step: number; wage_min: number; reserve_days: number };
  growth: { max_agents: number; arrivals_per_day: number } | null;
  agents: number;
  start_money: number;
  traits: string[];
}

/** Fetches JSON; a server error (e.g. a broken world file) becomes the thrown message. */
export async function getJson<T>(path: string): Promise<T> {
  const res = await fetch(path);
  const text = await res.text();
  if (!res.ok) throw new Error(text);
  return JSON.parse(text) as T;
}

export const fetchRules = () => getJson<Rules>('/api/rules');
