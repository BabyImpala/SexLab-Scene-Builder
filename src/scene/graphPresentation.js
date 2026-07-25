import {
  layoutSceneGraph,
  routeEdgesForPositions,
  filterEdgePlans,
  buildNodeSizes,
} from './graphLayout';
import {
  layoutFamilyClusters,
  visibleEdgeKeys,
  buildConnectionRows,
  shouldUseClusterLayout,
} from './graphLayoutClusters';
import { buildFamilyMap, isTransitionStage } from './stageFamily';
import { buildSpanningForest, layoutFromForest } from './spanningForest';
import { primaryEdgeKeys } from './edgeRanker';
import {
  buildCollapseProjection,
  degreeMaps,
  shortTransitionLabel,
} from './transitionCollapse';
import {
  viaEdgeAttrs,
  edgeLabelConfig,
  forwardEdgeAttrs,
} from './SceneEdge';
import './layoutPolicy.js';

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
 * transition collapse, family clustering, spanning-forest layout.
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
  collapseTransitions = true,
} = {}) {
  const nameOf = getName || ((id) => id);
  const fullIds = nodeIds?.length ? nodeIds : Object.keys(sceneGraph || {});

  const collapse = buildCollapseProjection(sceneGraph, {
    stages,
    getName: nameOf,
    enabled: !!collapseTransitions,
  });

  const viewGraph = collapse.poseGraph;
  const ids = collapse.visibleIds;
  const useCluster =
    preferCluster == null ? shouldUseClusterLayout(ids.length) : !!preferCluster;
  const filterMode = edgeMode ?? (useCluster ? 'neighborhood' : 'all');

  const forest = buildSpanningForest(viewGraph, rootId, ids, {
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
    const clustered = layoutFamilyClusters(viewGraph, rootId, ids, {
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
        dest: (viewGraph[id]?.dest || []).filter((t) =>
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
          const g = viewGraph[id] || sceneGraph[id] || {};
          return [id, { x: Number(g.x) || 40, y: Number(g.y) || 40 }];
        })
      );
    if (useCluster) {
      const clustered = layoutFamilyClusters(viewGraph, rootId, ids, {
        getName: nameOf,
      });
      families = clustered.families;
      clusters = clustered.clusters;
      hubReturnCounts = clustered.hubReturnCounts;
    } else {
      families = forest.families || buildFamilyMap(ids, nameOf);
    }
  }

  const { inCount, outCount } = degreeMaps(viewGraph);
  const stageById = new Map((stages || []).map((s) => [s.id, s]));
  const nodeSizes = buildNodeSizes(ids, inCount, outCount, (id) =>
    isTransitionStage(stageById.get(id) || nameOf(id))
  );

  const routed = routeEdgesForPositions(viewGraph, rootId, ids, positions, {
    isDark,
    nodeSizes,
    getName: nameOf,
  });

  const viaByKey = new Map(
    collapse.poseEdges
      .filter((e) => e.viaStageId)
      .map((e) => [`${e.source}\0${e.target}`, e])
  );

  const allEdges = (seededEdges || routed.edges).map((plan) => {
    const key = `${plan.source}\0${plan.target}`;
    const info = forest.edgeInfo.get(key);
    const via = viaByKey.get(key);
    const base = {
      ...plan,
      semanticRank: info?.rank || 'secondary',
      semanticScore: info?.score ?? 0,
      inTree: forest.treeKeys.has(key),
      viaStageId: via?.viaStageId || null,
      viaName: via?.viaName || null,
    };
    if (via?.viaStageId) {
      const label = shortTransitionLabel(via.viaName || via.viaStageId);
      return {
        ...base,
        attrs: viaEdgeAttrs(isDark),
        labels: edgeLabelConfig(label, isDark),
        kind: plan.kind === 'back' ? 'back' : 'via',
      };
    }
    return base;
  });

  const plannedKeys = new Set(allEdges.map((e) => `${e.source}\0${e.target}`));
  for (const pe of collapse.poseEdges) {
    const key = `${pe.source}\0${pe.target}`;
    if (plannedKeys.has(key)) continue;
    const via = pe.viaStageId;
    allEdges.push({
      source: pe.source,
      target: pe.target,
      kind: via ? 'via' : 'forward',
      sourcePort: 'out0',
      targetPort: 'in0',
      router: { name: 'normal' },
      connector: { name: 'rounded', args: { radius: 20 } },
      vertices: [],
      attrs: via ? viaEdgeAttrs(isDark) : forwardEdgeAttrs(isDark),
      labels: via
        ? edgeLabelConfig(shortTransitionLabel(pe.viaName), isDark)
        : [],
      viaStageId: pe.viaStageId,
      viaName: pe.viaName,
      semanticRank: 'secondary',
      semanticScore: 0,
      inTree: false,
    });
  }

  const ranks = seededRanks || routed.ranks;

  const { visibleKeys } = resolveVisibleKeys({
    sceneGraph: viewGraph,
    nodeIds: ids,
    edgeMode: filterMode,
    focusNodeIds,
    forest,
    ranks,
  });

  const connectionRows = buildRows
    ? buildConnectionRows(viewGraph, ids, {
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
    collapse,
    visibleIds: ids,
    hiddenIds: collapse.hiddenIds,
    inCount,
    outCount,
    nodeSizes,
    collapseTransitions: !!collapseTransitions,
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
