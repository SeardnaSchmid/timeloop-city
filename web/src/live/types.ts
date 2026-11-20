// JSON of the live engine (`sim serve`): /api/layout, /api/state, /api/history, /api/explain.

export interface Layout {
  w: number;
  h: number;
  landBase: number;
  landRadius: number;
  /** Last game minute of the run. */
  end: number;
  agents: number;
  /** Action names; an agent's `act` indexes this. */
  actions: string[];
  historyFields: string[];
  traits: string[];
}

export interface Business {
  name: string;
  good: string;
  x: number;
  y: number;
  slots: number;
  open: boolean;
  /** Agent id, -1 for the town. */
  owner: number;
  cash: number;
  price: number;
  wage: number;
  workers: number;
  since: number;
}

export interface House {
  name: string;
  x: number;
  y: number;
  beds: number;
  owner: number;
  /** Under construction. */
  building: boolean;
}

export interface Association {
  name: string;
  cash: number;
  members: number;
  rules: [string, number][];
}

export interface WorldState {
  now: number;
  hash: string;
  seed: number;
  epoch: number;
  days: number;
  employed: number;
  homeless: number;
  money: number;
  /** AGENT_FIELDS numbers per agent, see `agentAt`. */
  agents: number[];
  businesses: Business[];
  houses: House[];
  population: number;
  immigrants: number;
  housesBuilt: number;
  comforts: number;
  beds: number;
  rent: number[];
  tenants: number[];
  firms: number;
  deaths: number;
  births: number;
  newcomers: number;
  meanAge: number;
  inEducation: number;
  retired: number;
  levels: number[];
  meanLevel: number;
  priceIndex: number;
  associations: Association[];
  ages: number[];
  stages: number[];
  parents: number[];
  generations: number[];
  traits: number[][];
  /** Land price per parcel, row by row. */
  land: number[];
}

export type Speed = number | 'max';

export interface EngineState {
  playing: boolean;
  speed: Speed;
  /** Measured game minutes per real second. */
  effective: number;
  seeking: number | null;
  end: number;
  world: WorldState;
}

export interface HistoryChunk {
  epoch: number;
  from: number;
  /** historyFields.length numbers per day. */
  rows: number[];
}

export interface Explain {
  vars: [string, number][];
  candidates: { kind: string; score: number; allowed: string; formula: string }[];
  winner: string;
}

export const AGENT_FIELDS = 7;

export interface Agent {
  x: number;
  y: number;
  act: number;
  hunger: number;
  money: number;
  home: number;
  job: number;
}

export const people = (s: EngineState) => s.world.agents.length / AGENT_FIELDS;

export function agentAt(s: EngineState, k: number): Agent {
  const a = s.world.agents, o = k * AGENT_FIELDS;
  return { x: a[o] / 10, y: a[o + 1] / 10, act: a[o + 2], hunger: a[o + 3], money: a[o + 4], home: a[o + 5], job: a[o + 6] };
}

export type Selection = { type: 'agent' | 'house' | 'business'; id: number } | null;
