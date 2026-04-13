import React, { useEffect, useRef, useMemo, useState, useCallback } from 'react';
import {
  forceSimulation,
  forceLink,
  forceManyBody,
  forceCenter,
  forceCollide,
  type SimulationNodeDatum,
  type SimulationLinkDatum,
} from 'd3-force';
import { zoom as d3Zoom, zoomIdentity, type D3ZoomEvent } from 'd3-zoom';
import { select } from 'd3-selection';

/* ── Types ──────────────────────────────────────────────────────────── */

interface KgRelationInfo {
  relation: string;
  other_name: string;
  other_type: string;
  direction: string;
  confidence: number;
}

interface KgEntityInfo {
  id: string;
  name: string;
  entity_type: string;
  description: string | null;
  relations: KgRelationInfo[];
}

interface Props {
  entities: KgEntityInfo[];
  entityTypeColors: Record<string, string>;
}

interface GraphNode extends SimulationNodeDatum {
  id: string;
  entity: KgEntityInfo;
  radius: number;
  color: string;
}

interface GraphLink extends SimulationLinkDatum<GraphNode> {
  label: string;
  confidence: number;
  key: string;
}

/* ── Helpers ─────────────────────────────────────────────────────────── */

const BASE_RADIUS = 8;
const SCALE_FACTOR = 4;
const MAX_RADIUS = 28;

function nodeRadius(relCount: number): number {
  return Math.min(BASE_RADIUS + Math.sqrt(relCount) * SCALE_FACTOR, MAX_RADIUS);
}

/* ── Component ───────────────────────────────────────────────────────── */

export const KnowledgeGraph: React.FC<Props> = ({ entities, entityTypeColors }) => {
  const svgRef = useRef<SVGSVGElement>(null);
  const simRef = useRef<ReturnType<typeof forceSimulation<GraphNode>> | null>(null);
  const zoomRef = useRef<ReturnType<typeof d3Zoom<SVGSVGElement, unknown>> | null>(null);

  const [hovered, setHovered] = useState<string | null>(null);
  const [selected, setSelected] = useState<KgEntityInfo | null>(null);
  const [, forceRender] = useState(0);
  const tickCounter = useRef(0);

  // Build graph data from entities
  const { nodes, links } = useMemo(() => {
    const nodeMap = new Map<string, GraphNode>();
    const nodes: GraphNode[] = [];

    for (const e of entities) {
      const n: GraphNode = {
        id: e.id,
        entity: e,
        radius: nodeRadius(e.relations.length),
        color: entityTypeColors[e.entity_type] || '#6B7280',
      };
      nodes.push(n);
      nodeMap.set(e.name, n);
    }

    const links: GraphLink[] = [];
    const seen = new Set<string>();

    for (const e of entities) {
      const srcNode = nodeMap.get(e.name);
      if (!srcNode) continue;
      for (const rel of e.relations) {
        const tgtNode = nodeMap.get(rel.other_name);
        if (!tgtNode) continue;
        // Deduplicate bidirectional
        const pairKey = [srcNode.id, tgtNode.id].sort().join('::') + '::' + rel.relation;
        if (seen.has(pairKey)) continue;
        seen.add(pairKey);
        links.push({
          source: srcNode,
          target: tgtNode,
          label: rel.relation.replace(/_/g, ' '),
          confidence: rel.confidence,
          key: pairKey,
        });
      }
    }

    return { nodes, links };
  }, [entities, entityTypeColors]);

  // Connected node set for hover highlight
  const connectedTo = useMemo(() => {
    if (!hovered) return null;
    const set = new Set<string>();
    set.add(hovered);
    for (const l of links) {
      const s = (l.source as GraphNode).id;
      const t = (l.target as GraphNode).id;
      if (s === hovered) set.add(t);
      if (t === hovered) set.add(s);
    }
    return set;
  }, [hovered, links]);

  // Run simulation
  useEffect(() => {
    if (!svgRef.current || nodes.length === 0) return;

    const svg = svgRef.current;
    const width = svg.clientWidth || 600;
    const height = svg.clientHeight || 400;

    const sim = forceSimulation<GraphNode>(nodes)
      .force(
        'link',
        forceLink<GraphNode, GraphLink>(links)
          .id(d => d.id)
          .distance(l => 60 + (1 - l.confidence) * 40),
      )
      .force('charge', forceManyBody().strength(-150))
      .force('center', forceCenter(width / 2, height / 2))
      .force('collide', forceCollide<GraphNode>().radius(d => d.radius + 6))
      .alphaDecay(0.02);

    sim.on('tick', () => {
      tickCounter.current += 1;
      // Re-render React every 3 ticks for smoothness without excess renders
      if (tickCounter.current % 3 === 0 || sim.alpha() < 0.05) {
        forceRender(c => c + 1);
      }
    });

    simRef.current = sim;

    return () => {
      sim.stop();
      simRef.current = null;
    };
  }, [nodes, links]);

  // Zoom / pan
  useEffect(() => {
    if (!svgRef.current) return;
    const svg = select(svgRef.current);

    const zoomBehavior = d3Zoom<SVGSVGElement, unknown>()
      .scaleExtent([0.2, 5])
      .on('zoom', (event: D3ZoomEvent<SVGSVGElement, unknown>) => {
        const g = svg.select<SVGGElement>('g.zoom-group');
        g.attr('transform', event.transform.toString());
      });

    svg.call(zoomBehavior);
    zoomRef.current = zoomBehavior;

    // Fit to content after initial simulation settles
    const timer = setTimeout(() => {
      if (!svgRef.current) return;
      const width = svgRef.current.clientWidth || 600;
      const height = svgRef.current.clientHeight || 400;
      svg.call(zoomBehavior.transform, zoomIdentity.translate(width / 2, height / 2).scale(0.8).translate(-width / 2, -height / 2));
    }, 800);

    return () => {
      clearTimeout(timer);
      svg.on('.zoom', null);
    };
  }, []);

  // Drag handlers
  const onDragStart = useCallback(
    (e: React.MouseEvent, node: GraphNode) => {
      e.stopPropagation();
      const sim = simRef.current;
      if (!sim) return;

      sim.alphaTarget(0.3).restart();
      node.fx = node.x;
      node.fy = node.y;

      const onMove = (ev: MouseEvent) => {
        // Get SVG-space coordinates accounting for zoom
        const svg = svgRef.current;
        if (!svg) return;
        const g = svg.querySelector('g.zoom-group') as SVGGraphicsElement | null;
        if (!g) return;
        const pt = svg.createSVGPoint();
        pt.x = ev.clientX;
        pt.y = ev.clientY;
        const ctm = g.getScreenCTM();
        if (!ctm) return;
        const svgPt = pt.matrixTransform(ctm.inverse());
        node.fx = svgPt.x;
        node.fy = svgPt.y;
      };

      const onUp = () => {
        sim.alphaTarget(0);
        node.fx = null;
        node.fy = null;
        window.removeEventListener('mousemove', onMove);
        window.removeEventListener('mouseup', onUp);
      };

      window.addEventListener('mousemove', onMove);
      window.addEventListener('mouseup', onUp);
    },
    [],
  );

  const selectedEntity = selected;

  return (
    <div style={{ width: '100%', height: '100%', position: 'relative' }}>
      <svg
        ref={svgRef}
        style={{ width: '100%', height: '100%', cursor: 'grab' }}
      >
        <g className="zoom-group">
          {/* Edges */}
          {links.map(l => {
            const s = l.source as GraphNode;
            const t = l.target as GraphNode;
            if (s.x == null || t.x == null) return null;
            const dimmed = connectedTo && !connectedTo.has(s.id) && !connectedTo.has(t.id);
            const mx = ((s.x ?? 0) + (t.x ?? 0)) / 2;
            const my = ((s.y ?? 0) + (t.y ?? 0)) / 2;
            return (
              <g key={l.key} opacity={dimmed ? 0.1 : 0.6}>
                <line
                  x1={s.x}
                  y1={s.y}
                  x2={t.x}
                  y2={t.y}
                  stroke="var(--color-border-secondary)"
                  strokeWidth={1 + l.confidence}
                />
                <text
                  x={mx}
                  y={my}
                  textAnchor="middle"
                  dy={-4}
                  fontSize={9}
                  fill="var(--color-text-tertiary)"
                  pointerEvents="none"
                >
                  {l.label}
                </text>
              </g>
            );
          })}

          {/* Nodes */}
          {nodes.map(n => {
            if (n.x == null) return null;
            const dimmed = connectedTo && !connectedTo.has(n.id);
            return (
              <g
                key={n.id}
                transform={`translate(${n.x},${n.y})`}
                opacity={dimmed ? 0.12 : 1}
                style={{ cursor: 'pointer' }}
                onMouseEnter={() => setHovered(n.id)}
                onMouseLeave={() => setHovered(null)}
                onMouseDown={e => onDragStart(e, n)}
                onClick={() => setSelected(prev => prev?.id === n.entity.id ? null : n.entity)}
              >
                <circle
                  r={n.radius}
                  fill={n.color}
                  fillOpacity={0.85}
                  stroke={hovered === n.id ? '#fff' : 'transparent'}
                  strokeWidth={2}
                />
                <text
                  dy={n.radius + 12}
                  textAnchor="middle"
                  fontSize={11}
                  fontWeight={500}
                  fill="var(--color-text-primary)"
                  pointerEvents="none"
                >
                  {n.entity.name}
                </text>
              </g>
            );
          })}
        </g>
      </svg>

      {/* Legend */}
      <div
        style={{
          position: 'absolute',
          top: 8,
          left: 8,
          display: 'flex',
          gap: 8,
          flexWrap: 'wrap',
          fontSize: 10,
          color: 'var(--color-text-tertiary)',
          pointerEvents: 'none',
        }}
      >
        {Object.entries(entityTypeColors).map(([type, color]) => (
          <span key={type} style={{ display: 'flex', alignItems: 'center', gap: 3 }}>
            <span
              style={{
                width: 8,
                height: 8,
                borderRadius: '50%',
                background: color,
                display: 'inline-block',
              }}
            />
            {type}
          </span>
        ))}
      </div>

      {/* Detail panel */}
      {selectedEntity && (
        <div
          style={{
            position: 'absolute',
            bottom: 8,
            left: 8,
            right: 8,
            maxHeight: '40%',
            overflowY: 'auto',
            background: 'var(--color-background-secondary)',
            border: '1px solid var(--color-border-secondary)',
            borderRadius: 10,
            padding: '12px 14px',
            fontSize: 12,
          }}
        >
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 6 }}>
            <span
              style={{
                width: 10,
                height: 10,
                borderRadius: '50%',
                background: entityTypeColors[selectedEntity.entity_type] || '#6B7280',
                flexShrink: 0,
              }}
            />
            <span style={{ fontWeight: 600, color: 'var(--color-text-primary)' }}>
              {selectedEntity.name}
            </span>
            <span
              style={{
                fontSize: 10,
                color: entityTypeColors[selectedEntity.entity_type] || 'var(--color-text-tertiary)',
                textTransform: 'uppercase',
                fontWeight: 600,
                letterSpacing: '0.05em',
              }}
            >
              {selectedEntity.entity_type}
            </span>
            <button
              onClick={() => setSelected(null)}
              style={{
                marginLeft: 'auto',
                background: 'none',
                border: 'none',
                color: 'var(--color-text-tertiary)',
                cursor: 'pointer',
                fontSize: 16,
                lineHeight: 1,
              }}
            >
              ×
            </button>
          </div>
          {selectedEntity.description && (
            <div style={{ color: 'var(--color-text-secondary)', marginBottom: 8 }}>
              {selectedEntity.description}
            </div>
          )}
          {selectedEntity.relations.length > 0 && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 3 }}>
              {selectedEntity.relations.map((rel, i) => (
                <div
                  key={i}
                  style={{
                    display: 'flex',
                    gap: 4,
                    color: 'var(--color-text-secondary)',
                    fontSize: 11,
                  }}
                >
                  <span style={{ color: 'var(--color-text-tertiary)' }}>
                    {rel.direction === 'out' ? '→' : '←'}
                  </span>
                  <span style={{ color: entityTypeColors[rel.other_type] || 'var(--color-text-tertiary)' }}>
                    {rel.relation.replace(/_/g, ' ')}
                  </span>
                  <span style={{ fontWeight: 500 }}>{rel.other_name}</span>
                  <span style={{ color: 'var(--color-text-tertiary)', marginLeft: 'auto' }}>
                    {Math.round(rel.confidence * 100)}%
                  </span>
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
};
