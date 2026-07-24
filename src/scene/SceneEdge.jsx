import { Graph } from '@antv/x6'

export const STAGE_EDGE = {
  router: {
    name: "orth",
    args: {
      padding: 10,
    },
  },
  connector: "rounded",
  attrs: {
    line: {
      stroke: "#000",
      targetMarker: {
        name: 'block',
        width: 8,
        height: 6,
      },
    }
  }
}

export const STAGE_EDGE_SHAPEID = 'stage_edge';

Graph.registerEdge(
  STAGE_EDGE_SHAPEID,
  STAGE_EDGE,
  true
);