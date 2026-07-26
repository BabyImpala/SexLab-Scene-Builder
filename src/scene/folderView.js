/**
 * Virtual folder canvases: mount one ostim_folder subset at a time.
 * Cross-folder links become portal nodes + bridge edges (IR stays whole-scene).
 */

export const PORTAL_PREFIX = '__slsb_portal__:';

/** @param {string} id */
export function isPortalNodeId(id) {
  return typeof id === 'string' && id.startsWith(PORTAL_PREFIX);
}

/** @param {string} stageId */
export function portalIdForStage(stageId) {
  return `${PORTAL_PREFIX}${stageId}`;
}

/** @param {string} portalId */
export function stageIdFromPortal(portalId) {
  if (!isPortalNodeId(portalId)) return null;
  return portalId.slice(PORTAL_PREFIX.length);
}

/**
 * After transition collapse, restrict the pose graph to one folder and
 * replace cross-folder endpoints with portal stubs.
 *
 * @param {{
 *   poseGraph: Record<string, { dest?: string[], x?: number, y?: number }>,
 *   poseEdges: Array<{ source: string, target: string, viaStageId?: string|null, viaName?: string|null }>,
 *   folderFilter: string,
 *   folderMap: Map<string, string> | null,
 *   getName?: (id: string) => string,
 * }} args
 */
export function buildFolderViewProjection({
  poseGraph,
  poseEdges,
  folderFilter,
  folderMap,
  getName = null,
} = {}) {
  const nameOf = (id) => getName?.(id) || id;
  if (!folderFilter || folderFilter === 'all') {
    const ids = Object.keys(poseGraph || {});
    return {
      active: false,
      poseGraph,
      poseEdges: (poseEdges || []).map((e) => ({
        ...e,
        bridgeTargetId: null,
        bridgeSourceId: null,
        bridgeFolder: null,
        kind: e.viaStageId ? 'via' : 'forward',
      })),
      visibleIds: ids,
      realIds: ids,
      portalMeta: new Map(),
    };
  }

  const inView = (id) => (folderMap?.get(id) || '') === folderFilter;
  const visibleReal = Object.keys(poseGraph || {}).filter(inView);
  /** @type {Map<string, { stageId: string, folder: string, name: string }>} */
  const portalMeta = new Map();
  /** @type {Array<object>} */
  const newEdges = [];
  /** @type {Map<string, string[]>} */
  const destMap = new Map(visibleReal.map((id) => [id, []]));

  const ensurePortal = (stageId) => {
    const pid = portalIdForStage(stageId);
    if (!portalMeta.has(pid)) {
      portalMeta.set(pid, {
        stageId,
        folder: folderMap?.get(stageId) || '(other)',
        name: nameOf(stageId),
      });
    }
    if (!destMap.has(pid)) destMap.set(pid, []);
    return pid;
  };

  for (const e of poseEdges || []) {
    const sIn = inView(e.source);
    const tIn = inView(e.target);
    if (sIn && tIn) {
      newEdges.push({
        ...e,
        bridgeTargetId: null,
        bridgeSourceId: null,
        bridgeFolder: null,
        kind: e.viaStageId ? 'via' : 'forward',
      });
      destMap.get(e.source).push(e.target);
    } else if (sIn && !tIn) {
      const pid = ensurePortal(e.target);
      const folder = portalMeta.get(pid).folder;
      newEdges.push({
        source: e.source,
        target: pid,
        viaStageId: e.viaStageId || null,
        viaName: e.viaName || null,
        bridgeTargetId: e.target,
        bridgeSourceId: null,
        bridgeFolder: folder,
        kind: 'bridge',
      });
      destMap.get(e.source).push(pid);
    } else if (!sIn && tIn) {
      const pid = ensurePortal(e.source);
      const folder = portalMeta.get(pid).folder;
      newEdges.push({
        source: pid,
        target: e.target,
        viaStageId: e.viaStageId || null,
        viaName: e.viaName || null,
        bridgeTargetId: null,
        bridgeSourceId: e.source,
        bridgeFolder: folder,
        kind: 'bridge',
      });
      destMap.get(pid).push(e.target);
    }
  }

  let maxX = 40;
  let minY = 40;
  for (const id of visibleReal) {
    const p = poseGraph[id];
    maxX = Math.max(maxX, (Number(p?.x) || 40) + 240);
    minY = Math.min(minY, Number(p?.y) || 40);
  }

  /** @type {Record<string, { dest: string[], x: number, y: number }>} */
  const newGraph = {};
  for (const id of visibleReal) {
    newGraph[id] = {
      dest: [...new Set(destMap.get(id) || [])],
      x: Number(poseGraph[id]?.x) || 40,
      y: Number(poseGraph[id]?.y) || 40,
    };
  }

  const byFolder = new Map();
  for (const [pid, meta] of portalMeta) {
    if (!byFolder.has(meta.folder)) byFolder.set(meta.folder, []);
    byFolder.get(meta.folder).push(pid);
  }

  let folderCol = 0;
  for (const [, pids] of [...byFolder.entries()].sort((a, b) =>
    a[0].localeCompare(b[0])
  )) {
    pids.sort((a, b) =>
      (portalMeta.get(a)?.name || a).localeCompare(portalMeta.get(b)?.name || b)
    );
    pids.forEach((pid, i) => {
      newGraph[pid] = {
        dest: [...new Set(destMap.get(pid) || [])],
        x: maxX + 40 + folderCol * 210,
        y: minY + i * 88,
      };
    });
    folderCol += 1;
  }

  return {
    active: true,
    poseGraph: newGraph,
    poseEdges: newEdges,
    visibleIds: [...visibleReal, ...portalMeta.keys()],
    realIds: visibleReal,
    portalMeta,
  };
}

/**
 * Stage ids that belong to the active folder view (for graph sync merge).
 * @param {Map<string, string>|null} folderMap
 * @param {string} folderFilter
 * @param {string[]} allStageIds
 */
export function folderViewStageIds(folderMap, folderFilter, allStageIds) {
  if (!folderFilter || folderFilter === 'all') return null;
  return (allStageIds || []).filter(
    (id) => (folderMap?.get(id) || '') === folderFilter
  );
}
