import {
  layoutSceneGraph,
  routeEdgesForPositions,
  filterEdgePlans,
} from './graphLayout';
import {
  layoutFamilyClusters,
  visibleEdgeKeys,
  buildConnectionRows,
  shouldUseClusterLayout,
} from './graphLayoutClusters';
import { buildFamilyMap } from './stageFamily';
import { buildSpanningForest, layoutFromForest } from './spanningForest';
import { primaryEdgeKeys } from './edgeRanker';
import './layoutPolicy.js'; // documents SLSB-vs-OStim coordinate rules

/**
 * Compact signature of topology + positions. Used to invalidate presentation cache.
 * @param {object} sceneGraph
 */
export function sceneGraphSignature(sceneGraph) {
  const ids = Object.keys(sceneGraph || {}).sort();
  let s = `${ids.length}|`;
  for (const id of ids) {
    const n = sceneGraph[id] || {};
    const dest = (n.dest || []).slice().sort().join(',');
    s += `${id}:${Math.round(n.x) || 0},${Math.round(n.y) || 0}>${dest};`;
  }
  return s;
}

/**
 * Resolve visible edge keys from a cached forest (no re-rank / re-route).
 */
export function resolveVisibleKeys({
  sceneGraph,
  nodeIds,
  edgeMode,
  focusNodeIds = [],
  familyFilter = 'all',
  forest,
  ranks = null,
}) {
  const ids = nodeIds?.length ? nodeIds : Object.keys(sceneGraph || {});
  const families = forest?.families || buildFamilyMap(ids, (id) => id);
  const mode = edgeMode || 'all';

  let visibleKeys =
    mode === 'all'
      ? null
      : visibleEdgeKeys(mode, sceneGraph, ids, families, {
          focusNodeIds,
          ranks: ranks || forest?.ranks,
          treeKeys: forest?.treeKeys,
          edgeInfo: forest?.edgeInfo,
        });

  if (familyFilter && familyFilter !== 'all') {
    const filtered = new Set();
    const allKeys = [];
    for (const source of ids) {
      for (const target of sceneGraph[source]?.dest || []) {
        allKeys.push(`${source}\0${target}`);
      }
    }
    const base = visibleKeys || new Set(allKeys);
    for (const key of base) {
      const [s, t] = key.split('\0');
      if (families.get(s) === familyFilter && families.get(t) === familyFilter) {
        filtered.add(key);
      }
    }
    const focus = new Set((focusNodeIds || []).filter(Boolean));
    if (focus.size && mode === 'neighborhood') {
      for (const key of base) {
        const [s, t] = key.split('\0');
        if (focus.has(s) || focus.has(t)) filtered.add(key);
      }
    }
    visibleKeys = filtered;
  }

  return { visibleKeys, families };
}

/**
 * Dim nodes outside the active family filter (cheap).
 */
export function applyNodeFamilyDim(graph, families, familyFilter) {
  if (!graph) return;
  graph.getNodes().forEach((n) => {
    const pf = n.prop('poseFamily') || families?.get(n.id) || '';
    const dim =
      familyFilter && familyFilter !== 'all' && pf !== familyFilter;
    if (typeof n.setOpacity === 'function') {
      n.setOpacity(dim ? 0.2 : 1);
    } else {
      n.attr('body/opacity', dim ? 0.2 : 1);
    }
  });
}

/**
 * Compute positions + edge plans for the scene graph, with optional
 * family clustering, spanning-forest browse layout, and edge visibility.
 *
 * Always returns `allEdges` (full routing). `edges` is the visible subset.
 * Keep all edges on the X6 graph and toggle visibility so saves stay complete.
 *
 * Layout coordinates are editor/SLSB-only — never written into OStim scene JSON.
 */
export function computeGraphPresentation({
  sceneGraph,
  rootId,
  nodeIds,
  getName,
  isDark = false,
  edgeMode = null,
  focusNodeIds = [],
  preferCluster = null,
  existingPositions = null,
  rearrange = true,
  stages = [],
  useForestLayout = true,
  buildRows = false,
} = {}) {
  const ids = nodeIds?.length ? nodeIds : Object.keys(sceneGraph || {});
  const nameOf = getName || ((id) => id);
  const useCluster =
    preferCluster == null ? shouldUseClusterLayout(ids.length) : !!preferCluster;
  const filterMode = edgeMode ?? (useCluster ? 'neighborhood' : 'all');

  const forest = buildSpanningForest(sceneGraph, rootId, ids, {
    getName: nameOf,
    stages,
  });

  let positions;
  let families = forest.families;
  let clusters = [];
  let hubReturnCounts = forest.secondaryInbound || new Map();
  let seededEdges = null;
  let seededRanks = forest.ranks;

  if (rearrange && useForestLayout && !useCluster) {
    positions = layoutFromForest(forest.ranks, ids, {
      families: forest.families,
      children: forest.children,
      roots: forest.roots,
      getName: nameOf,
    });
    families = forest.families;
  } else if (rearrange && useCluster) {
    const clustered = layoutFamilyClusters(sceneGraph, rootId, ids, {
      getName: nameOf,
    });
    positions = clustered.positions;
    families = clustered.families;
    clusters = clustered.clusters;
    hubReturnCounts = clustered.hubReturnCounts;
  } else if (rearrange) {
    const treeGraph = {};
    for (const id of ids) {
      treeGraph[id] = {
        dest: (sceneGraph[id]?.dest || []).filter((t) =>
          forest.treeKeys.has(`${id}\0${t}`)
        ),
        x: 0,
        y: 0,
      };
    }
    const layout = layoutSceneGraph(treeGraph, rootId, ids, { isDark });
    positions = layout.positions;
    families = forest.families || buildFamilyMap(ids, nameOf);
    seededEdges = null;
    seededRanks = layout.ranks;
  } else {
    positions =
      existingPositions ||
      new Map(
        ids.map((id) => {
          const g = sceneGraph[id] || {};
          return [id, { x: Number(g.x) || 40, y: Number(g.y) || 40 }];
        })
      );
    if (useCluster) {
      const clustered = layoutFamilyClusters(sceneGraph, rootId, ids, {
        getName: nameOf,
      });
      families = clustered.families;
      clusters = clustered.clusters;
      hubReturnCounts = clustered.hubReturnCounts;
    } else {
      families = forest.families || buildFamilyMap(ids, nameOf);
    }
  }

  const routed = routeEdgesForPositions(sceneGraph, rootId, ids, positions, {
    isDark,
  });
  const allEdges = (seededEdges || routed.edges).map((plan) => {
    const key = `${plan.source}\0${plan.target}`;
    const info = forest.edgeInfo.get(key);
    return {
      ...plan,
      semanticRank: info?.rank || 'secondary',
      semanticScore: info?.score ?? 0,
      inTree: forest.treeKeys.has(key),
    };
  });
  const ranks = seededRanks || routed.ranks;

  const { visibleKeys } = resolveVisibleKeys({
    sceneGraph,
    nodeIds: ids,
    edgeMode: filterMode,
    focusNodeIds,
    forest,
    ranks,
  });

  const connectionRows = buildRows
    ? buildConnectionRows(sceneGraph, ids, {
        getName: nameOf,
        families,
        ranks,
        edgeInfo: forest.edgeInfo,
        treeKeys: forest.treeKeys,
      })
    : [];

  return {
    positions,
    edges: filterEdgePlans(allEdges, visibleKeys),
    allEdges,
    visibleKeys,
    ranks,
    families,
    clusters,
    hubReturnCounts,
    useCluster,
    filterMode,
    connectionRows,
    forest,
    outline: forest.outline,
    treeKeys: forest.treeKeys,
    primaryKeys: primaryEdgeKeys(forest.edgeInfo),
    signature: sceneGraphSignature(sceneGraph),
  };
}

export function applyEdgeVisibility(graph, visibleKeys) {
  if (!graph) return;
  graph.getEdges().forEach((edge) => {
    const s = edge.getSourceCellId();
    const t = edge.getTargetCellId();
    const key = `${s}\0${t}`;
    const show = !visibleKeys || visibleKeys.has(key);
    if (typeof edge.setVisible === 'function') {
      edge.setVisible(show);
    } else {
      edge.setProp('visible', show);
    }
  });
}
