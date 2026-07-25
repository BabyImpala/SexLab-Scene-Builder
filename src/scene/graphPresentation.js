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
import { buildStageLookups, navMetaForEdge } from './edgeRanker';
import './layoutPolicy.js';

/**
 * Prefer OStim nav description (e.g. "bow down"); never use icon/border.
 */
function viaEdgeLabelText(sourceStage, viaStage, viaName, ostimToStage) {
  const meta = navMetaForEdge(sourceStage, viaStage, ostimToStage);
  const desc = String(meta?.description || '').trim();
  if (desc) return desc;
  return shortTransitionLabel(viaName || viaStage?.name || '');
}

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
const LAYER_DIM_NODE = 0.4;
const LAYER_DIM_EDGE_OPACITY = 0.4;
const LAYER_DIM_EDGE_WIDTH = 1.25;

function cloneLineAttrs(line) {
  if (!line || typeof line !== 'object') return {};
  const out = { ...line };
  if (line.targetMarker && typeof line.targetMarker === 'object') {
    out.targetMarker = { ...line.targetMarker };
  }
  if (line.sourceMarker && typeof line.sourceMarker === 'object') {
    out.sourceMarker = { ...line.sourceMarker };
  }
  return out;
}

/** Restore full opacity — X6 merges attrs, so dim keys must be cleared explicitly. */
function activeLineAttrs(base) {
  const line = cloneLineAttrs(base);
  line.strokeOpacity = 1;
  line.opacity = 1;
  if (line.strokeWidth == null) line.strokeWidth = 1.75;
  return line;
}

function dimmedLineAttrs(base) {
  const line = cloneLineAttrs(base);
  line.strokeOpacity = LAYER_DIM_EDGE_OPACITY;
  line.opacity = LAYER_DIM_EDGE_OPACITY;
  line.strokeWidth = Math.min(Number(line.strokeWidth) || 1.75, LAYER_DIM_EDGE_WIDTH);
  return line;
}

export function applyNodeFamilyDim(graph, families, familyFilter) {
  if (!graph) return;
  graph.getNodes().forEach((n) => {
    const pf = n.prop('poseFamily') || families?.get(n.id) || '';
    const familyDim =
      familyFilter && familyFilter !== 'all' && pf !== familyFilter;
    const layerDim = !!n.prop('layerDim');
    let opacity = 1;
    if (layerDim) opacity = Math.min(opacity, LAYER_DIM_NODE);
    if (familyDim) opacity = Math.min(opacity, 0.2);
    if (typeof n.setOpacity === 'function') {
      n.setOpacity(opacity);
    } else {
      n.attr('body/opacity', opacity);
    }
  });
}

/**
 * Dim/hide inactive graph layer for Poses / Transitions modes.
 * Inactive nodes use setVisible(false) — WebKitGTK foreignObject ignores parent
 * opacity, so setOpacity alone does not hide pose nodes reliably.
 * `mode`: 'collapsed' | 'poses' | 'transitions'
 */
export function applyGraphLayerDim(graph, mode = 'collapsed') {
  if (!graph) return;

  const run = () => {
    const wantTransitionActive = mode === 'transitions';
    const layerOn = mode === 'poses' || mode === 'transitions';

    graph.getNodes().forEach((n) => {
      const isT = !!n.prop('isTransition');
      const active = !layerOn || (wantTransitionActive ? isT : !isT);
      n.setProp('layerDim', !active, { silent: true });
      if (typeof n.setVisible === 'function') {
        n.setVisible(active);
      } else {
        n.setProp('visible', active);
      }
      // Opacity as a soft fallback for non-WebKit; FO may ignore it.
      if (typeof n.setOpacity === 'function') {
        n.setOpacity(active ? 1 : LAYER_DIM_NODE);
      }
      n.setZIndex?.(active ? (layerOn ? 4 : 1) : 1);
    });

    graph.getEdges().forEach((e) => {
      if (!layerOn) {
        e.setProp('layerDim', false, { silent: true });
        const base = e.prop('layerBaseAttrs')?.line;
        e.attr('line', activeLineAttrs(base || e.attr('line') || {}));
        e.setZIndex?.(0);
        const filterShow = e.prop('filterVisible') !== false;
        if (typeof e.setVisible === 'function') e.setVisible(filterShow);
        else e.setProp('visible', filterShow);
        return;
      }

      const s = e.getSourceCell();
      const t = e.getTargetCell();
      const sT = !!s?.prop?.('isTransition');
      const tT = !!t?.prop?.('isTransition');
      const touchesT = sT || tT;
      const active = wantTransitionActive ? touchesT : !touchesT;
      const wasDim = !!e.prop('layerDim');
      const filterShow = e.prop('filterVisible') !== false;
      const wantVisible = filterShow && active;

      if (wasDim === !active) {
        const vis =
          typeof e.isVisible === 'function' ? e.isVisible() : e.prop('visible') !== false;
        if (vis === wantVisible) return;
      }

      let base = e.prop('layerBaseAttrs')?.line;
      if (!base) {
        base = cloneLineAttrs(e.attr('line') || {});
        base.strokeOpacity = 1;
        base.opacity = 1;
        e.setProp('layerBaseAttrs', { line: cloneLineAttrs(base) }, { silent: true });
      }

      e.setProp('layerDim', !active, { silent: true });
      if (active) {
        e.attr('line', activeLineAttrs(base));
        e.setZIndex?.(3);
      } else {
        e.attr('line', dimmedLineAttrs(base));
        e.setZIndex?.(0);
      }
      if (typeof e.setVisible === 'function') {
        e.setVisible(wantVisible);
      } else {
        e.setProp('visible', wantVisible);
      }
    });
  };

  if (typeof graph.startBatch === 'function') {
    graph.startBatch('layer-dim');
    try {
      run();
    } finally {
      graph.stopBatch('layer-dim');
    }
  } else {
    run();
  }
}

/**
 * Place a node that isn't on the canvas yet (typical: transition stages after
 * expanding from Collapsed). Prefer midpoint of placed neighbors over stored defaults.
 */
function midpointFromNeighbors(id, sceneGraph, placed) {
  const g = sceneGraph?.[id] || {};
  const neighborIds = [];
  for (const [sid, node] of Object.entries(sceneGraph || {})) {
    if ((node?.dest || []).includes(id)) neighborIds.push(sid);
  }
  for (const t of g.dest || []) neighborIds.push(t);

  const pts = [];
  for (const nid of neighborIds) {
    const p = placed.get(nid);
    if (p && Number.isFinite(p.x) && Number.isFinite(p.y)) {
      pts.push(p);
      continue;
    }
    const ng = sceneGraph[nid] || {};
    const nx = Number(ng.x);
    const ny = Number(ng.y);
    if (
      Number.isFinite(nx) &&
      Number.isFinite(ny) &&
      !(nx === 40 && ny === 40) &&
      !(nx === 0 && ny === 0)
    ) {
      pts.push({ x: nx, y: ny });
    }
  }
  if (!pts.length) return null;
  return {
    x: pts.reduce((s, p) => s + p.x, 0) / pts.length,
    y: pts.reduce((s, p) => s + p.y, 0) / pts.length,
  };
}

function positionForMissingNode(id, sceneGraph, placed) {
  const mid = midpointFromNeighbors(id, sceneGraph, placed);
  if (mid) return mid;
  const g = sceneGraph?.[id] || {};
  const sx = Number(g.x);
  const sy = Number(g.y);
  return {
    x: Number.isFinite(sx) ? sx : 40,
    y: Number.isFinite(sy) ? sy : 40,
  };
}

/** Nudge nodes that share nearly the same coordinates. */
function nudgeOverlappingNodes(positions, ids) {
  const list = ids.filter((id) => positions.has(id));
  for (let i = 0; i < list.length; i++) {
    const a = positions.get(list[i]);
    if (!a) continue;
    let slot = 0;
    for (let j = i + 1; j < list.length; j++) {
      const b = positions.get(list[j]);
      if (!b) continue;
      if (Math.abs(a.x - b.x) < 24 && Math.abs(a.y - b.y) < 24) {
        slot += 1;
        positions.set(list[j], {
          x: a.x + slot * 56,
          y: a.y + slot * 44,
        });
      }
    }
  }
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
    const filled = new Map(existingPositions || []);
    /** @type {Map<string, number>} */
    const midOccupancy = new Map();
    for (const id of ids) {
      const cur = filled.get(id);
      const isT = isTransitionStage(nameOf(id));
      if (isT) {
        const mid = midpointFromNeighbors(id, sceneGraph, filled);
        if (mid) {
          const key = `${Math.round(mid.x / 8)},${Math.round(mid.y / 8)}`;
          const slot = midOccupancy.get(key) || 0;
          midOccupancy.set(key, slot + 1);
          filled.set(id, {
            x: mid.x + slot * 56,
            y: mid.y + slot * 44,
          });
        } else if (!cur) {
          filled.set(id, positionForMissingNode(id, sceneGraph, filled));
        }
      } else if (!cur) {
        filled.set(id, positionForMissingNode(id, sceneGraph, filled));
      }
    }
    nudgeOverlappingNodes(filled, ids);
    positions = filled;
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

  const { byId: stageLookup, ostimToStage } = buildStageLookups(stages || []);

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
      const label = viaEdgeLabelText(
        stageLookup.get(plan.source),
        stageLookup.get(via.viaStageId),
        via.viaName || via.viaStageId,
        ostimToStage
      );
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
    const label = via
      ? viaEdgeLabelText(
          stageLookup.get(pe.source),
          stageLookup.get(via),
          pe.viaName,
          ostimToStage
        )
      : '';
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
      labels: via ? edgeLabelConfig(label, isDark) : [],
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
    edge.setProp('filterVisible', show, { silent: true });
    const layerDim = !!edge.prop('layerDim');
    const visible = show && !layerDim;
    if (typeof edge.setVisible === 'function') {
      edge.setVisible(visible);
    } else {
      edge.setProp('visible', visible);
    }
  });
}
