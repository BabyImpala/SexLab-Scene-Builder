import { cleanStageName, isTransitionStage } from './stageFamily';

export function shortTransitionLabel(name) {
  const cleaned = cleanStageName(name || '');
  return cleaned.replace(/^Go to\s+/i, '') || cleaned || 'transition';
}

/**
 * @param {object} sceneGraph
 * @param {{ stages?: object[], getName?: (id: string) => string, enabled?: boolean }} [opts]
 */
export function buildCollapseProjection(
  sceneGraph,
  { stages = [], getName = null, enabled = true } = {}
) {
  const stageById = new Map((stages || []).map((s) => [s.id, s]));
  const ids = Object.keys(sceneGraph || {});
  const nameOf = (id) => getName?.(id) || stageById.get(id)?.name || id;

  const inbound = new Map(ids.map((id) => [id, []]));
  for (const s of ids) {
    for (const t of sceneGraph[s]?.dest || []) {
      if (!inbound.has(t)) inbound.set(t, []);
      inbound.get(t).push(s);
    }
  }

  const isTrans = (id) =>
    isTransitionStage(stageById.get(id) || nameOf(id));

  /** @type {Set<string>} */
  const hiddenIds = new Set();
  /** @type {Array<{ source: string, target: string, viaStageId: string|null, viaName: string|null }>} */
  const poseEdges = [];
  /** @type {Map<string, string>} */
  const viaByPoseEdge = new Map();

  if (!enabled) {
    for (const s of ids) {
      for (const t of sceneGraph[s]?.dest || []) {
        poseEdges.push({
          source: s,
          target: t,
          viaStageId: null,
          viaName: null,
        });
      }
    }
    return {
      hiddenIds,
      poseEdges,
      poseGraph: sceneGraph,
      visibleIds: ids,
      viaByPoseEdge,
    };
  }

  for (const id of ids) {
    if (!isTrans(id)) continue;
    const outs = sceneGraph[id]?.dest || [];
    const ins = inbound.get(id) || [];
    if (outs.length !== 1 || ins.length !== 1) continue;
    const a = ins[0];
    const c = outs[0];
    if (!ids.includes(a) || !ids.includes(c)) continue;
    if (isTrans(a) || isTrans(c)) continue;
    hiddenIds.add(id);
    const key = `${a}\0${c}`;
    viaByPoseEdge.set(key, id);
    poseEdges.push({
      source: a,
      target: c,
      viaStageId: id,
      viaName: nameOf(id),
    });
  }

  for (const s of ids) {
    if (hiddenIds.has(s)) continue;
    for (const t of sceneGraph[s]?.dest || []) {
      if (hiddenIds.has(t)) continue;
      const key = `${s}\0${t}`;
      if (viaByPoseEdge.has(key)) continue;
      poseEdges.push({
        source: s,
        target: t,
        viaStageId: null,
        viaName: null,
      });
    }
  }

  const visibleIds = ids.filter((id) => !hiddenIds.has(id));
  const poseGraph = {};
  for (const id of visibleIds) {
    const dest = [];
    for (const e of poseEdges) {
      if (e.source === id) dest.push(e.target);
    }
    poseGraph[id] = {
      dest: [...new Set(dest)],
      x: Number(sceneGraph[id]?.x) || 40,
      y: Number(sceneGraph[id]?.y) || 40,
    };
  }

  return {
    hiddenIds,
    poseEdges,
    poseGraph,
    visibleIds,
    viaByPoseEdge,
  };
}

/**
 * Rebuild full SLSB graph from canvas pose nodes/edges + prior full graph.
 * Via-labeled edges expand to A→T→C; hidden transition stages are kept.
 *
 * @param {{
 *   stages: object[],
 *   prevGraph: object,
 *   nodes: Array<{ id: string, x: number, y: number }>,
 *   edges: Array<{ source: string, target: string, viaStageId?: string|null }>,
 * }} args
 */
export function expandCanvasToStoredGraph({
  stages,
  prevGraph = {},
  nodes = [],
  edges = [],
}) {
  const next = {};
  for (const stage of stages || []) {
    const prev = prevGraph[stage.id] || {};
    next[stage.id] = {
      dest: [],
      x: Number(prev.x) || 40,
      y: Number(prev.y) || 40,
    };
  }

  const nodePos = new Map(nodes.map((n) => [n.id, n]));
  for (const [id, pos] of nodePos) {
    if (!next[id]) {
      next[id] = { dest: [], x: pos.x, y: pos.y };
    } else {
      next[id].x = pos.x;
      next[id].y = pos.y;
    }
  }

  for (const edge of edges) {
    const s = edge.source;
    const t = edge.target;
    const via = edge.viaStageId;
    if (!s || !t || !next[s]) continue;
    if (via && next[via]) {
      if (!next[s].dest.includes(via)) next[s].dest.push(via);
      if (!next[via].dest.includes(t)) next[via].dest.push(t);
      const sp = next[s];
      const tp = next[t];
      if (sp && tp) {
        next[via].x = (sp.x + tp.x) / 2;
        next[via].y = (sp.y + tp.y) / 2;
      }
    } else if (next[t] || true) {
      if (!next[s].dest.includes(t)) next[s].dest.push(t);
    }
  }

  return next;
}

/**
 * Degree counts for visible pose graph (for slot sizing).
 * @returns {{ inCount: Map<string, number>, outCount: Map<string, number> }}
 */
export function degreeMaps(poseGraph) {
  const ids = Object.keys(poseGraph || {});
  const inCount = new Map(ids.map((id) => [id, 0]));
  const outCount = new Map(ids.map((id) => [id, 0]));
  for (const id of ids) {
    const dest = poseGraph[id]?.dest || [];
    outCount.set(id, dest.length);
    for (const t of dest) {
      inCount.set(t, (inCount.get(t) || 0) + 1);
    }
  }
  return { inCount, outCount };
}
