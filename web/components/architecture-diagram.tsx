'use client';

import '@xyflow/react/dist/style.css';
import {
  Background,
  BackgroundVariant,
  type Edge,
  Handle,
  MarkerType,
  type Node,
  type NodeProps,
  Position,
  ReactFlow,
} from '@xyflow/react';
import { useTheme } from 'next-themes';
import { useEffect, useState, type CSSProperties } from 'react';

type ArchData = { title: string; subtitle: string };

function ArchNode({ data }: NodeProps<Node<ArchData>>) {
  return (
    <div
      className="rounded-[3px] border border-fd-border bg-fd-background px-4 py-3 font-mono text-xs shadow-sm"
      style={{ width: 230 }}
    >
      <Handle type="target" position={Position.Top} style={{ opacity: 0 }} />
      <div className="flex items-center gap-2 font-semibold text-fd-foreground">
        <span className="size-1.5 shrink-0" style={{ background: 'var(--color-accent)' }} aria-hidden />
        {data.title}
      </div>
      <div className="mt-1.5 leading-relaxed text-fd-muted-foreground">{data.subtitle}</div>
      <Handle type="source" position={Position.Bottom} style={{ opacity: 0 }} />
    </div>
  );
}

const nodeTypes = { arch: ArchNode };

const nodes: Node<ArchData>[] = [
  {
    id: 'cli',
    type: 'arch',
    position: { x: 165, y: 0 },
    data: { title: 'CLI / TUI', subtitle: 'src/' },
  },
  {
    id: 'app',
    type: 'arch',
    position: { x: 0, y: 140 },
    data: {
      title: 'uze-application',
      subtitle: 'orchestration: add · install · remove · update · context',
    },
  },
  {
    id: 'integ',
    type: 'arch',
    position: { x: 330, y: 140 },
    data: {
      title: 'uze-integrations',
      subtitle: 'Claude Code · Codex · OpenCode · Antigravity',
    },
  },
  {
    id: 'core',
    type: 'arch',
    position: { x: 165, y: 280 },
    data: {
      title: 'uze-core',
      subtitle: 'Package · Store · Engine · Router · IntegrationPort',
    },
  },
];

const edges: Edge[] = [
  { id: 'cli-app', source: 'cli', target: 'app' },
  { id: 'app-core', source: 'app', target: 'core' },
  { id: 'integ-core', source: 'integ', target: 'core' },
].map((edge) => ({
  ...edge,
  type: 'smoothstep',
  markerEnd: { type: MarkerType.ArrowClosed, width: 16, height: 16 },
}));

export function ArchitectureDiagram() {
  const { resolvedTheme } = useTheme();
  // next-themes only knows the real theme after mount — resolvedTheme is
  // undefined on both the server render and the client's first (hydration)
  // pass. Rendering 'light' until then keeps that first pass identical on
  // both sides; ReactFlow's colorMode-driven className only flips to the
  // real theme in a later, non-hydration render.
  const [mounted, setMounted] = useState(false);
  useEffect(() => setMounted(true), []);
  const colorMode = mounted && resolvedTheme === 'dark' ? 'dark' : 'light';

  return (
    <div
      className="my-6 h-[380px] rounded-md border border-fd-border"
      style={
        {
          '--xy-background-color': 'var(--color-paper)',
          '--xy-background-pattern-color': 'var(--color-line)',
          '--xy-edge-stroke': 'var(--color-accent)',
          '--xy-edge-stroke-width': '1.5',
          '--xy-controls-button-background-color': 'var(--color-surface)',
          '--xy-controls-button-color': 'var(--color-ink)',
          '--xy-controls-button-border-color': 'var(--color-line)',
        } as CSSProperties
      }
    >
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        colorMode={colorMode}
        nodesDraggable={false}
        nodesConnectable={false}
        edgesFocusable={false}
        fitView
        fitViewOptions={{ padding: 0.25 }}
      >
        <Background variant={BackgroundVariant.Dots} gap={16} size={1} />
      </ReactFlow>
    </div>
  );
}
