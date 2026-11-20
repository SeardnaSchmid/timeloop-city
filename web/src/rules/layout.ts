// Automatic left-to-right layout of the value chain with ELK.
import ELK from 'elkjs/lib/elk.bundled.js';
import type { ChainEdge, ChainNode, NodeKind } from './graph';

export const SIZE: Record<NodeKind, { w: number; h: number }> = {
  good: { w: 120, h: 40 },
  building: { w: 210, h: 78 },
  sink: { w: 200, h: 78 },
  need: { w: 190, h: 52 },
};

const elk = new ELK();

export async function layout(nodes: ChainNode[], edges: ChainEdge[]) {
  const res = await elk.layout({
    id: 'root',
    layoutOptions: {
      'elk.algorithm': 'layered',
      'elk.direction': 'RIGHT',
      'elk.spacing.nodeNode': '18',
      'elk.layered.spacing.nodeNodeBetweenLayers': '60',
      'elk.layered.considerModelOrder.strategy': 'NODES_AND_EDGES',
      'elk.layered.nodePlacement.strategy': 'NETWORK_SIMPLEX',
      'elk.layered.crossingMinimization.forceNodeModelOrder': 'true',
      'elk.layered.layering.strategy': 'LONGEST_PATH_SOURCE',
    },
    children: nodes.map((n) => ({ id: n.id, width: SIZE[n.kind].w, height: SIZE[n.kind].h })),
    edges: edges.map((e) => ({ id: e.id, sources: [e.source], targets: [e.target] })),
  });
  const pos = new Map<string, { x: number; y: number }>();
  for (const c of res.children ?? []) pos.set(c.id, { x: c.x ?? 0, y: c.y ?? 0 });
  return pos;
}
