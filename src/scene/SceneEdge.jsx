import { Graph } from '@antv/x6'

/** Keep radius ≤ half of the shortest planned orth segment (~48px). */
export const ROUNDED_CONNECTOR = {
  name: 'rounded',
  args: { radius: 20 },
};

export const EDGE_MARKER = {
  name: 'block',
  width: 14,
  height: 10,
};

export function forwardEdgeAttrs(isDark = false) {
  return {
    line: {
      stroke: isDark ? '#e5e5e5' : '#111111',
      strokeWidth: 1.75,
      targetMarker: { ...EDGE_MARKER },
    },
  };
}

export function backEdgeAttrs(isDark = false) {
  return {
    line: {
      stroke: isDark ? '#fb923c' : '#c2410c',
      strokeWidth: 2,
      targetMarker: { ...EDGE_MARKER },
    },
  };
}

export const STAGE_EDGE = {
  router: {
    name: 'orth',
    args: {
      padding: 10,
    },
  },
  connector: ROUNDED_CONNECTOR,
  attrs: forwardEdgeAttrs(false),
};

export const STAGE_EDGE_SHAPEID = 'stage_edge';

Graph.registerEdge(
  STAGE_EDGE_SHAPEID,
  STAGE_EDGE,
  true
);
