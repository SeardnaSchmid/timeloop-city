// The value chain drawn with React Flow; positions come from ELK.
import { useEffect, useState } from 'react';
import { Background, Controls, Handle, MarkerType, Position, ReactFlow, type Edge, type Node, type NodeProps } from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import type { Rules } from '../api';
import { buildGraph, type ChainNode } from './graph';
import { layout, SIZE } from './layout';
import './chain.css';

type Data = ChainNode & Record<string, unknown>;

function ChainBox({ data }: NodeProps<Node<Data>>) {
  const { w, h } = SIZE[data.kind];
  return (
    <div className={`chain-node ${data.kind} ${data.role ?? ''}`} style={{ width: w, minHeight: h }} title={data.tip}>
      <Handle type="target" position={Position.Left} />
      <div className="title">{data.title}</div>
      {data.lines.map((l) => (
        <div key={l} className="line">
          {l}
        </div>
      ))}
      <Handle type="source" position={Position.Right} />
    </div>
  );
}

const nodeTypes = { box: ChainBox };
const color = { town: 'var(--town)', recipe: 'var(--recipe)', need: 'var(--need)', mat: 'var(--mat)' };

export function ValueChain({ rules }: { rules: Rules }) {
  const [graph, setGraph] = useState<{ nodes: Node<Data>[]; edges: Edge[] } | null>(null);
  useEffect(() => {
    const { nodes, edges } = buildGraph(rules);
    let live = true;
    layout(nodes, edges).then((pos) => {
      if (!live) return;
      setGraph({
        nodes: nodes.map((n) => ({ id: n.id, type: 'box', position: pos.get(n.id)!, data: n as Data, draggable: true })),
        edges: edges.map((e) => ({
          id: e.id,
          source: e.source,
          target: e.target,
          label: e.label,
          style: { stroke: color[e.role], strokeDasharray: e.role === 'need' || e.role === 'mat' ? '5 4' : undefined },
          labelStyle: { fill: color[e.role], fontSize: 11 },
          labelBgStyle: { fill: 'var(--panel)' },
          markerEnd: { type: MarkerType.ArrowClosed, color: color[e.role] },
        })),
      });
    });
    return () => {
      live = false;
    };
  }, [rules]);
  if (!graph) return <div className="muted">laying out…</div>;
  return (
    <div className="chain">
      <ReactFlow
        nodes={graph.nodes}
        edges={graph.edges}
        nodeTypes={nodeTypes}
        fitView
        minZoom={0.2}
        nodesConnectable={false}
        proOptions={{ hideAttribution: true }}
      >
        <Background gap={24} size={1} />
        <Controls showInteractive={false} />
      </ReactFlow>
    </div>
  );
}
