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
import {
  primaryEdgeKeys,
  buildStageLookups,
  navMetaForEdge,
  parseNavText,
} from './edgeRanker';
import {
  buildCollapseProjection,
  degreeMaps,
  shortTransitionLabel,
} from './transitionCollapse';
import {
  viaEdgeAttrs,
  bridgeEdgeAttrs,
  edgeLabelConfig,
  forwardEdgeAttrs,
} from './SceneEdge';
import { NODE_HEIGHT, NODE_WIDTH } from './SceneNode';
import {
  buildFolderViewProjection,
  isPortalNodeId,
} from './folderView';
import './layoutPolicy.js';

/**
 * Label on a collapsed transition edge = that stage's NavText.
 * OStim-encoded nav_text uses descriptions; freeform NavText is shown as-is.
 */
function viaEdgeLabelText(sourceStage, viaStage, viaName, ostimToStage) {
  const raw = String(viaStage?.extra?.nav_text || '').trim();
  if (raw) {
    const entries = parseNavText(raw);
    if (entries.length) {
      const descs = entries
        .map((e) => String(e.description || '').trim())
        .filter(Boolean);
      if (descs.length) return descs.join(' · ');
    } else {
      return raw;
    }
  }
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
  folderFilter = 'all',
  folderMap = null,
  neighborhoodSet = null,
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

  const allKeys = [];
  for (const source of ids) {
    for (const target of sceneGraph[source]?.dest || []) {
      allKeys.push(`${source}\0${target}`);
    }
  }
  let base = visibleKeys || new Set(allKeys);
  const focus = new Set((focusNodeIds || []).filter(Boolean));

  const inFolder = (id) => {
    if (!folderFilter || folderFilter === 'all') return true;
    const f = folderMap?.get(id) || '';
    return f === folderFilter;
  };
  const inNeighborhood = (id) => {
    if (!neighborhoodSet) return true;
    return neighborhoodSet.has(id);
  };

  if (
    (familyFilter && familyFilter !== 'all') ||
    (folderFilter && folderFilter !== 'all') ||
    neighborhoodSet
  ) {
    const filtered = new Set();
    for (const key of base) {
      const [s, t] = key.split('\0');
      const familyOk =
        !familyFilter ||
        familyFilter === 'all' ||
        (families.get(s) === familyFilter && families.get(t) === familyFilter);
      const folderOk = inFolder(s) && inFolder(t);
      const neighOk = inNeighborhood(s) && inNeighborhood(t);
      if (familyOk && folderOk && neighOk) {
        filtered.add(key);
      } else if (focus.size && (focus.has(s) || focus.has(t))) {
        // Keep incident edges on the focused node when filters would hide them,
        // unless neighborhood mode excludes the far endpoint.
        if (!neighborhoodSet || (inNeighborhood(s) && inNeighborhood(t))) {
          if (!folderFilter || folderFilter === 'all' || inFolder(s) || inFolder(t)) {
            filtered.add(key);
          }
        }
      }
    }
    visibleKeys = filtered;
  }

  return { visibleKeys, families };
}

/**
 * Dim nodes outside active family / folder / neighborhood filters.
 */
export function applyNodeFocusDim(
  graph,
  {
    families = null,
    familyFilter = 'all',
    folderMap = null,
    folderFilter = 'all',
    neighborhoodSet = null,
  } = {}
) {
  if (!graph) return;
  graph.getNodes().forEach((n) => {
    const pf = n.prop('poseFamily') || families?.get(n.id) || '';
    const familyDim =
      familyFilter && familyFilter !== 'all' && pf !== familyFilter;
    const folder = folderMap?.get(n.id) || n.prop('ostimFolder') || '';
    const folderDim =
      folderFilter && folderFilter !== 'all' && folder !== folderFilter;
    const neighDim = neighborhoodSet && !neighborhoodSet.has(n.id);
    const opacity = familyDim || folderDim || neighDim ? 0.18 : 1;
    if (typeof n.setOpacity === 'function') {
      n.setOpacity(opacity);
    } else {
      n.attr('body/opacity', opacity);
    }
  });
}

/** @deprecated use applyNodeFocusDim */
export function applyNodeFamilyDim(graph, families, familyFilter) {
  applyNodeFocusDim(graph, { families, familyFilter });
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
function nudgeOverlappingNodes(positions, ids, nodeSizes = null) {
  const heightOf = (id) =>
    Math.max(NODE_HEIGHT, Number(nodeSizes?.get(id)?.height) || NODE_HEIGHT);
  const widthOf = (id) =>
    Math.max(NODE_WIDTH, Number(nodeSizes?.get(id)?.width) || NODE_WIDTH);
  const list = ids.filter((id) => positions.has(id));
  // Stable top-to-bottom, left-to-right so we push later nodes down/right.
  list.sort((a, b) => {
    const pa = positions.get(a);
    const pb = positions.get(b);
    if (pa.y !== pb.y) return pa.y - pb.y;
    return pa.x - pb.x;
  });
  for (let i = 0; i < list.length; i++) {
    const aId = list[i];
    const a = positions.get(aId);
    if (!a) continue;
    const ah = heightOf(aId);
    const aw = widthOf(aId);
    for (let j = i + 1; j < list.length; j++) {
      const bId = list[j];
      const b = positions.get(bId);
      if (!b) continue;
      const bh = heightOf(bId);
      const bw = widthOf(bId);
      const overlapX = a.x < b.x + bw && a.x + aw > b.x;
      const overlapY = a.y < b.y + bh && a.y + ah > b.y;
      if (!overlapX || !overlapY) continue;
      positions.set(bId, {
        x: b.x,
        y: a.y + ah + 56,
      });
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
  familyFilter = 'all',
  folderFilter = 'all',
  folderMap = null,
  neighborhoodSet = null,
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

  const folderView = buildFolderViewProjection({
    poseGraph: collapse.poseGraph,
    poseEdges: collapse.poseEdges,
    folderFilter,
    folderMap,
    getName: nameOf,
  });

  const viewGraph = folderView.poseGraph;
  const ids = folderView.visibleIds;
  const nameOfView = (id) => {
    if (isPortalNodeId(id)) {
      const meta = folderView.portalMeta.get(id);
      const folder = meta?.folder || '?';
      const stageName = meta?.name || meta?.stageId || id;
      return `→ ${folder}: ${stageName}`;
    }
    return nameOf(id);
  };
  const useCluster =
    preferCluster == null ? shouldUseClusterLayout(ids.length) : !!preferCluster;
  const filterMode = edgeMode ?? (useCluster ? 'neighborhood' : 'all');

  const forest = buildSpanningForest(viewGraph, rootId, ids, {
    getName: nameOfView,
    stages,
  });

  const { inCount, outCount } = degreeMaps(viewGraph);
  const stageById = new Map((stages || []).map((s) => [s.id, s]));
  const nodeSizes = buildNodeSizes(ids, inCount, outCount, (id) =>
    isPortalNodeId(id) ||
    isTransitionStage(stageById.get(id) || nameOf(id))
  );

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
      nodeSizes,
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
    const layout = layoutSceneGraph(treeGraph, rootId, ids, {
      isDark,
      nodeSizes,
    });
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
    nudgeOverlappingNodes(filled, ids, nodeSizes);
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

  const routed = routeEdgesForPositions(viewGraph, rootId, ids, positions, {
    isDark,
    nodeSizes,
    getName: nameOfView,
  });

  // Prefer folder-view edge list (includes portals); fall back to collapse.
  const viewPoseEdges = folderView.poseEdges || collapse.poseEdges;
  const edgeMetaByKey = new Map(
    viewPoseEdges.map((e) => [`${e.source}\0${e.target}`, e])
  );

  const { byId: stageLookup, ostimToStage } = buildStageLookups(stages || []);

  const decoratePlan = (plan, meta) => {
    const key = `${plan.source}\0${plan.target}`;
    const info = forest.edgeInfo.get(key);
    const viaStageId = meta?.viaStageId || null;
    const viaName = meta?.viaName || null;
    const bridgeTargetId = meta?.bridgeTargetId || null;
    const bridgeSourceId = meta?.bridgeSourceId || null;
    const bridgeFolder = meta?.bridgeFolder || null;
    const isBridge = !!(bridgeTargetId || bridgeSourceId || meta?.kind === 'bridge');
    const base = {
      ...plan,
      semanticRank: info?.rank || 'secondary',
      semanticScore: info?.score ?? 0,
      inTree: forest.treeKeys.has(key),
      viaStageId,
      viaName,
      bridgeTargetId,
      bridgeSourceId,
      bridgeFolder,
    };
    if (isBridge) {
      const destName = bridgeTargetId
        ? nameOf(bridgeTargetId)
        : bridgeSourceId
          ? nameOf(bridgeSourceId)
          : '';
      const label = bridgeFolder
        ? `→ ${bridgeFolder}${destName ? `: ${destName}` : ''}`
        : destName
          ? `→ ${destName}`
          : '→ other folder';
      return {
        ...base,
        attrs: bridgeEdgeAttrs(isDark),
        labels: edgeLabelConfig(label, isDark),
        kind: 'bridge',
      };
    }
    if (viaStageId) {
      const label = viaEdgeLabelText(
        stageLookup.get(
          isPortalNodeId(plan.source) ? bridgeSourceId : plan.source
        ),
        stageLookup.get(viaStageId),
        viaName || viaStageId,
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
  };

  const allEdges = (seededEdges || routed.edges).map((plan) => {
    const key = `${plan.source}\0${plan.target}`;
    return decoratePlan(plan, edgeMetaByKey.get(key));
  });

  const plannedKeys = new Set(allEdges.map((e) => `${e.source}\0${e.target}`));
  for (const pe of viewPoseEdges) {
    const key = `${pe.source}\0${pe.target}`;
    if (plannedKeys.has(key)) continue;
    allEdges.push(
      decoratePlan(
        {
          source: pe.source,
          target: pe.target,
          kind: 'forward',
          sourcePort: 'out0',
          targetPort: 'in0',
          router: { name: 'normal' },
          connector: { name: 'rounded', args: { radius: 20 } },
          vertices: [],
          attrs: forwardEdgeAttrs(isDark),
          labels: [],
          slotOut: 'out0',
          slotIn: 'in0',
        },
        pe
      )
    );
  }

  const ranks = seededRanks || routed.ranks;

  // Folder is structural (virtual canvas); do not also dim/filter by folder.
  const { visibleKeys } = resolveVisibleKeys({
    sceneGraph: viewGraph,
    nodeIds: ids,
    edgeMode: filterMode,
    focusNodeIds,
    familyFilter,
    folderFilter: folderView.active ? 'all' : folderFilter,
    folderMap,
    neighborhoodSet,
    forest,
    ranks,
  });

  const connectionRows = buildRows
    ? buildConnectionRows(viewGraph, ids, {
        getName: nameOfView,
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
    folderView,
    visibleIds: ids,
    realIds: folderView.realIds || ids,
    portalMeta: folderView.portalMeta,
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
    // Preview / half-connected edges must stay visible during click-to-connect.
    if (edge.getData?.()?.preview) {
      if (typeof edge.setVisible === 'function') edge.setVisible(true);
      else edge.setProp('visible', true);
      edge.setProp('filterVisible', true, { silent: true });
      return;
    }
    const s = edge.getSourceCellId();
    const t = edge.getTargetCellId();
    if (!s || !t) {
      if (typeof edge.setVisible === 'function') edge.setVisible(true);
      else edge.setProp('visible', true);
      edge.setProp('filterVisible', true, { silent: true });
      return;
    }
    const key = `${s}\0${t}`;
    // null / undefined = show all (folder canvases remount a subset already).
    const show = visibleKeys == null || visibleKeys.has(key);
    edge.setProp('filterVisible', show, { silent: true });
    if (typeof edge.setVisible === 'function') {
      edge.setVisible(show);
    } else {
      edge.setProp('visible', show);
    }
    // X6 can leave opacity/display stuck after setVisible(false) across remounts.
    if (show) {
      try {
        edge.attr('line/opacity', 1);
        edge.attr('line/strokeOpacity', 1);
      } catch (_) { /* ignore */ }
    }
  });
}
