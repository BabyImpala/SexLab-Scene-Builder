import { NODE_WIDTH, NODE_HEIGHT, OUT_PORT_BY_SIDE, IN_PORT_BY_SIDE } from './SceneNode';
import {
  STAGE_EDGE_SHAPEID,
  ROUNDED_CONNECTOR,
  forwardEdgeAttrs,
  backEdgeAttrs,
} from './SceneEdge';

const ORIGIN = 40;
const MAX_STAGES_PER_ROW = 5;
const MAX_PER_COLUMN = 8;
/** Clearance past the node edge before the first bend. */
const STUB = 56;
const STUB_STEP = 12;
const LANE_GAP = 36;
const BACK_LANE_GAP = 56;
const TOP_LANE_GAP = 40;
const SUBCOL_GAP = 48;
/** Min orth segment so X6 rounded corners (r≈20) look clean. */
const MIN_SEG = 48;

function LanePool(start, gap) {
  let next = 0;
  return {
    take() {
      const v = start + next * gap;
      next += 1;
      return v;
    },
  };
}

/** True when all nodes share one origin or form a degenerate column/row. */
export function graphCoordsStacked(sceneGraph) {
  const positions = Object.values(sceneGraph || {}).map(({ x, y }) => ({
    x: Number(x) || 0,
    y: Number(y) || 0,
  }));
  if (positions.length < 2) return false;
  const first = positions[0];
  if (positions.every((p) => p.x === first.x && p.y === first.y)) return true;
  const sameX = positions.every((p) => p.x === first.x);
  const sameY = positions.every((p) => p.y === first.y);
  return sameX || sameY;
}

function buildAdjacency(sceneGraph, nodeIds) {
  const idSet = new Set(nodeIds);
  const outgoing = new Map();
  const incoming = new Map();
  nodeIds.forEach((id) => {
    outgoing.set(id, []);
    incoming.set(id, []);
  });
  nodeIds.forEach((id) => {
    const dest = (sceneGraph[id]?.dest || []).filter((d) => idSet.has(d));
    outgoing.set(id, dest);
    dest.forEach((d) => incoming.get(d).push(id));
  });
  return { outgoing, incoming };
}

function assignRanks(outgoing, nodeIds, rootId) {
  const start = nodeIds.includes(rootId) ? rootId : nodeIds[0];
  const ranks = new Map();
  const queue = start ? [start] : [];
  if (start) ranks.set(start, 0);
  while (queue.length) {
    const id = queue.shift();
    for (const dest of outgoing.get(id) || []) {
      if (!ranks.has(dest)) {
        ranks.set(dest, ranks.get(id) + 1);
        queue.push(dest);
      }
    }
  }
  const orphans = nodeIds.filter((id) => !ranks.has(id));
  return { ranks, orphans };
}

function orderByBarycenter(byLevel, outgoing, incoming) {
  const orderIndex = new Map();
  const syncOrder = () => {
    orderIndex.clear();
    for (const [, ids] of [...byLevel.entries()].sort((a, b) => a[0] - b[0])) {
      ids.forEach((id, i) => orderIndex.set(id, i));
    }
  };
  const avg = (ids) => {
    const vals = ids.map((id) => orderIndex.get(id)).filter((v) => v !== undefined);
    if (!vals.length) return Number.POSITIVE_INFINITY;
    return vals.reduce((a, b) => a + b, 0) / vals.length;
  };
  syncOrder();
  for (let pass = 0; pass < 4; pass++) {
    for (const [lv, ids] of [...byLevel.entries()].sort((a, b) => a[0] - b[0])) {
      if (lv === 0) continue;
      ids.sort((a, b) => avg(incoming.get(a) || []) - avg(incoming.get(b) || []));
      byLevel.set(lv, ids);
    }
    syncOrder();
    for (const [lv, ids] of [...byLevel.entries()].sort((a, b) => b[0] - a[0])) {
      ids.sort((a, b) => avg(outgoing.get(a) || []) - avg(outgoing.get(b) || []));
      byLevel.set(lv, ids);
    }
    syncOrder();
  }
}

function placeNodes(byLevel, orphans, nodeIds, ranks) {
  const large = nodeIds.length > 40;
  const hGap = large ? 500 : 400;
  const vGap = large ? 220 : 190;
  const maxCol = large ? MAX_PER_COLUMN : Math.max(MAX_PER_COLUMN, 12);

  const ordered = [];
  for (const [, ids] of [...byLevel.entries()].sort((a, b) => a[0] - b[0])) {
    ordered.push(...ids);
  }
  ordered.push(...orphans);

  const linear =
    orphans.length === 0 &&
    [...byLevel.values()].every((ids) => ids.length <= 1);

  const positions = new Map();

  if (linear && ordered.length > MAX_STAGES_PER_ROW) {
    ordered.forEach((id, i) => {
      const row = Math.floor(i / MAX_STAGES_PER_ROW);
      const col = i % MAX_STAGES_PER_ROW;
      positions.set(id, {
        x: ORIGIN + col * hGap,
        y: ORIGIN + row * vGap,
      });
    });
    return { positions, hGap, vGap };
  }

  let xCursor = ORIGIN;
  for (const [, ids] of [...byLevel.entries()].sort((a, b) => a[0] - b[0])) {
    const subCols = Math.max(1, Math.ceil(ids.length / maxCol));
    ids.forEach((id, i) => {
      const sub = Math.floor(i / maxCol);
      const row = i % maxCol;
      positions.set(id, {
        x: xCursor + sub * (NODE_WIDTH + SUBCOL_GAP),
        y: ORIGIN + row * vGap,
      });
    });
    xCursor += Math.max(hGap, subCols * (NODE_WIDTH + SUBCOL_GAP) + 56);
  }

  if (orphans.length) {
    const maxRows = Math.max(
      1,
      ...[...byLevel.values()].map((ids) => Math.min(ids.length, maxCol)),
      0
    );
    const orphanY = ORIGIN + maxRows * vGap + vGap;
    orphans.forEach((id, i) => {
      const row = Math.floor(i / MAX_STAGES_PER_ROW);
      const col = i % MAX_STAGES_PER_ROW;
      positions.set(id, {
        x: ORIGIN + col * hGap,
        y: orphanY + row * vGap,
      });
      if (!ranks.has(id)) ranks.set(id, -1);
    });
  }

  return { positions, hGap, vGap };
}

function portY(pos) {
  return pos.y + NODE_HEIGHT / 2;
}

function portX(pos) {
  return pos.x + NODE_WIDTH / 2;
}

/**
 * Prefer LTR sides. Top/bottom only when |dx| is small and |dy| is large.
 */
function chooseSides(sp, tp) {
  const dx = portX(tp) - portX(sp);
  const dy = portY(tp) - portY(sp);
  const stacked =
    Math.abs(dx) < NODE_WIDTH * 0.4 && Math.abs(dy) > NODE_HEIGHT * 0.9;
  if (stacked) {
    return {
      outSide: dy >= 0 ? 'bottom' : 'top',
      inSide: dy >= 0 ? 'top' : 'bottom',
    };
  }
  return {
    outSide: dx >= 0 ? 'right' : 'left',
    inSide: dx >= 0 ? 'left' : 'right',
  };
}

/** Stub just outside a node side. */
function sideStub(pos, side, stub, along = 0, role = 'out') {
  const cx = portX(pos);
  const cy = portY(pos);
  const ports = role === 'out' ? OUT_PORT_BY_SIDE : IN_PORT_BY_SIDE;
  switch (side) {
    case 'left':
      return { x: pos.x - stub, y: cy + along, port: ports.left };
    case 'right':
      return { x: pos.x + NODE_WIDTH + stub, y: cy + along, port: ports.right };
    case 'top':
      return { x: cx + along, y: pos.y - stub, port: ports.top };
    case 'bottom':
      return { x: cx + along, y: pos.y + NODE_HEIGHT + stub, port: ports.bottom };
    default:
      return { x: pos.x + NODE_WIDTH + stub, y: cy + along, port: ports.right };
  }
}

function routeBetweenStubs(exit, enter, outSide, inSide, midPrefer) {
  const ex = exit.x;
  const ey = exit.y;
  const ix = enter.x;
  const iy = enter.y;
  const outVert = outSide === 'top' || outSide === 'bottom';
  const inVert = inSide === 'top' || inSide === 'bottom';

  if (outVert && inVert) {
    if (Math.abs(ex - ix) < 8) {
      return simplifyOrtho([
        { x: ex, y: ey },
        { x: ix, y: iy },
      ]);
    }
    const midY =
      midPrefer != null ? midPrefer : (ey + iy) / 2;
    return simplifyOrtho([
      { x: ex, y: ey },
      { x: ex, y: midY },
      { x: ix, y: midY },
      { x: ix, y: iy },
    ]);
  }

  if (!outVert && !inVert) {
    if (Math.abs(ey - iy) < 8) {
      return simplifyOrtho([
        { x: ex, y: ey },
        { x: ix, y: iy },
      ]);
    }
    let midX = midPrefer != null ? midPrefer : (ex + ix) / 2;
    const lo = Math.min(ex, ix) + MIN_SEG * 0.5;
    const hi = Math.max(ex, ix) - MIN_SEG * 0.5;
    if (hi > lo) midX = Math.min(Math.max(midX, lo), hi);
    return simplifyOrtho([
      { x: ex, y: ey },
      { x: midX, y: ey },
      { x: midX, y: iy },
      { x: ix, y: iy },
    ]);
  }

  if (outVert) {
    return simplifyOrtho([
      { x: ex, y: ey },
      { x: ex, y: iy },
      { x: ix, y: iy },
    ]);
  }
  return simplifyOrtho([
    { x: ex, y: ey },
    { x: ix, y: ey },
    { x: ix, y: iy },
  ]);
}

function classifyEdge(sourceRank, targetRank) {
  // Provisional only — planEdges reclassifies by geometry for cyclic graphs.
  if (sourceRank < 0 || targetRank < 0) {
    if (targetRank >= 0 && sourceRank < 0) return 'forward';
    if (sourceRank >= 0 && targetRank < 0) return 'back';
    return 'same';
  }
  if (targetRank > sourceRank) return 'forward';
  if (targetRank < sourceRank) return 'back';
  return 'same';
}

/** Route kind from node placement. */
function geometricRouteKind(sp, tp) {
  const dx = tp.x - sp.x;
  const dy = tp.y - sp.y;
  if (dx < -NODE_WIDTH * 0.2) return 'back';
  if (Math.abs(dx) < 48 && Math.abs(dy) < NODE_HEIGHT * 0.35) return 'same';
  if (Math.abs(dx) < 48 && dy < 0) return 'back';
  return 'forward';
}

function collectEdges(outgoing, ranks) {
  const edges = [];
  for (const [source, dests] of outgoing) {
    for (const target of dests) {
      const sr = ranks.has(source) ? ranks.get(source) : -1;
      const tr = ranks.has(target) ? ranks.get(target) : -1;
      edges.push({
        source,
        target,
        kind: classifyEdge(sr, tr),
        sourceRank: sr,
        targetRank: tr,
      });
    }
  }
  return edges;
}

function indexBy(list, keyFn) {
  const map = new Map();
  for (const item of list) {
    const key = keyFn(item);
    if (!map.has(key)) map.set(key, []);
    map.get(key).push(item);
  }
  return map;
}

function pairBounds(sp, tp) {
  return {
    minX: Math.min(sp.x, tp.x),
    maxX: Math.max(sp.x + NODE_WIDTH, tp.x + NODE_WIDTH),
    minY: Math.min(sp.y, tp.y),
    maxY: Math.max(sp.y + NODE_HEIGHT, tp.y + NODE_HEIGHT),
  };
}

function simplifyOrtho(pts) {
  if (!pts?.length) return [];
  const out = [{ x: pts[0].x, y: pts[0].y }];
  for (let i = 1; i < pts.length; i++) {
    const cur = pts[i];
    const prev = out[out.length - 1];
    if (Math.abs(cur.x - prev.x) < 1 && Math.abs(cur.y - prev.y) < 1) continue;
    if (out.length >= 2) {
      const a = out[out.length - 2];
      const horiz = Math.abs(a.y - prev.y) < 1 && Math.abs(prev.y - cur.y) < 1;
      const vert = Math.abs(a.x - prev.x) < 1 && Math.abs(prev.x - cur.x) < 1;
      if (horiz || vert) {
        out[out.length - 1] = { x: cur.x, y: cur.y };
        continue;
      }
    }
    out.push({ x: cur.x, y: cur.y });
  }
  return out;
}

/** U under the pair; never first-moves left into the source. */
function underU(exitX, exitY, enterX, enterY, laneY, sidePrefer) {
  const floorY = Math.max(laneY, Math.max(exitY, enterY) + MIN_SEG);
  if (Math.abs(exitX - enterX) < MIN_SEG) {
    const sideX =
      sidePrefer != null
        ? Math.max(sidePrefer, exitX + MIN_SEG, enterX + MIN_SEG)
        : Math.max(exitX, enterX) + MIN_SEG;
    return simplifyOrtho([
      { x: exitX, y: exitY },
      { x: sideX, y: exitY },
      { x: sideX, y: enterY },
      { x: enterX, y: enterY },
    ]);
  }
  return simplifyOrtho([
    { x: exitX, y: exitY },
    { x: exitX, y: floorY },
    { x: enterX, y: floorY },
    { x: enterX, y: enterY },
  ]);
}

function overU(exitX, exitY, enterX, enterY, laneY, sidePrefer) {
  const ceilY = Math.min(laneY, Math.min(exitY, enterY) - MIN_SEG);
  if (Math.abs(exitX - enterX) < MIN_SEG) {
    const sideX =
      sidePrefer != null
        ? Math.max(sidePrefer, exitX + MIN_SEG, enterX + MIN_SEG)
        : Math.max(exitX, enterX) + MIN_SEG;
    return simplifyOrtho([
      { x: exitX, y: exitY },
      { x: sideX, y: exitY },
      { x: sideX, y: enterY },
      { x: enterX, y: enterY },
    ]);
  }
  return simplifyOrtho([
    { x: exitX, y: exitY },
    { x: exitX, y: ceilY },
    { x: enterX, y: ceilY },
    { x: enterX, y: enterY },
  ]);
}

function sideC(exitX, exitY, enterX, enterY, sideX) {
  const sx = Math.max(sideX, exitX + MIN_SEG, enterX + MIN_SEG);
  return simplifyOrtho([
    { x: exitX, y: exitY },
    { x: sx, y: exitY },
    { x: sx, y: enterY },
    { x: enterX, y: enterY },
  ]);
}

/** Ortho Z; falls back to under-U when enterX is not right of exitX. */
function gutterZ(exitX, exitY, enterX, enterY, midX) {
  if (enterX < exitX + MIN_SEG) {
    const floorY = Math.max(exitY, enterY) + MIN_SEG;
    return simplifyOrtho([
      { x: exitX, y: exitY },
      { x: exitX, y: floorY },
      { x: enterX, y: floorY },
      { x: enterX, y: enterY },
    ]);
  }
  const span = enterX - exitX;
  if (span < MIN_SEG * 2) {
    const floorY = Math.max(exitY, enterY) + MIN_SEG;
    return simplifyOrtho([
      { x: exitX, y: exitY },
      { x: exitX, y: floorY },
      { x: enterX, y: floorY },
      { x: enterX, y: enterY },
    ]);
  }
  let mx = midX;
  const lo = exitX + MIN_SEG * 0.5;
  const hi = enterX - MIN_SEG * 0.5;
  if (hi > lo) mx = Math.min(Math.max(mx, lo), hi);
  else mx = (exitX + enterX) / 2;
  if (Math.abs(enterY - exitY) < 8) {
    return simplifyOrtho([
      { x: exitX, y: exitY },
      { x: enterX, y: enterY },
    ]);
  }
  return simplifyOrtho([
    { x: exitX, y: exitY },
    { x: mx, y: exitY },
    { x: mx, y: enterY },
    { x: enterX, y: enterY },
  ]);
}

const OBS_PAD = 14;

function segmentHitsRect(a, b, r) {
  const eps = 0.5;
  if (Math.abs(a.y - b.y) < eps) {
    const y = a.y;
    if (y < r.y - eps || y > r.y + r.height + eps) return false;
    const minX = Math.min(a.x, b.x);
    const maxX = Math.max(a.x, b.x);
    return maxX > r.x + eps && minX < r.x + r.width - eps;
  }
  if (Math.abs(a.x - b.x) < eps) {
    const x = a.x;
    if (x < r.x - eps || x > r.x + r.width + eps) return false;
    const minY = Math.min(a.y, b.y);
    const maxY = Math.max(a.y, b.y);
    return maxY > r.y + eps && minY < r.y + r.height - eps;
  }
  return false;
}

function pathHitsObstacle(pts, obstacles) {
  if (!pts?.length || !obstacles?.length) return false;
  for (let i = 0; i < pts.length - 1; i++) {
    for (const obs of obstacles) {
      if (segmentHitsRect(pts[i], pts[i + 1], obs)) return true;
    }
  }
  return false;
}

function pickClearPath(candidates, obstacles) {
  if (!candidates?.length) return [];
  for (const pts of candidates) {
    if (!pathHitsObstacle(pts, obstacles)) return pts;
  }
  return candidates[candidates.length - 1];
}

function buildObstacleRects(positions, excludeIds) {
  const skip = new Set(excludeIds || []);
  const rects = [];
  for (const [id, pos] of positions) {
    if (skip.has(id) || !pos) continue;
    rects.push({
      id,
      x: pos.x - OBS_PAD,
      y: pos.y - OBS_PAD,
      width: NODE_WIDTH + OBS_PAD * 2,
      height: NODE_HEIGHT + OBS_PAD * 2,
    });
  }
  return rects;
}

function planEdges(rawEdges, positions, { isDark = false } = {}) {
  /** @type {Map<string, ReturnType<typeof LanePool>>} */
  const localUnderPools = new Map();
  /** @type {Map<string, ReturnType<typeof LanePool>>} */
  const localOverPools = new Map();
  /** @type {Map<string, ReturnType<typeof LanePool>>} */
  const gutterPools = new Map();
  /** @type {Map<string, number>} */
  const stubOutCount = new Map();
  /** @type {Map<string, number>} */
  const stubInCount = new Map();

  const keyPair = (a, b) => (a < b ? `${a}|${b}` : `${b}|${a}`);

  const takeLocalUnder = (sp, tp) => {
    const b = pairBounds(sp, tp);
    const key = `u:${Math.round(b.maxY / 48)}:${Math.round((b.minX + b.maxX) / 240)}`;
    if (!localUnderPools.has(key)) {
      localUnderPools.set(key, LanePool(b.maxY + MIN_SEG, BACK_LANE_GAP));
    }
    return { laneY: localUnderPools.get(key).take(), bounds: b };
  };

  const takeLocalOver = (sp, tp) => {
    const b = pairBounds(sp, tp);
    const key = `o:${Math.round(b.minY / 48)}:${Math.round((b.minX + b.maxX) / 240)}`;
    if (!localOverPools.has(key)) {
      localOverPools.set(
        key,
        LanePool(Math.max(8, b.minY - MIN_SEG), -TOP_LANE_GAP)
      );
    }
    return { laneY: localOverPools.get(key).take(), bounds: b };
  };

  const gutterLane = (key, baseX) => {
    if (!gutterPools.has(key)) gutterPools.set(key, LanePool(baseX, LANE_GAP));
    return gutterPools.get(key).take();
  };

  const peekStub = (map, id, side) => {
    const key = `${id}:${side}`;
    const i = map.get(key) || 0;
    return { stub: STUB + (i % 5) * STUB_STEP, along: 0, key, i };
  };
  const commitStub = (map, key, i) => {
    map.set(key, i + 1);
  };

  const forwardAttrs = forwardEdgeAttrs(isDark);
  const backAttrs = backEdgeAttrs(isDark);

  const classified = rawEdges.map((edge) => {
    const sp = positions.get(edge.source);
    const tp = positions.get(edge.target);
    if (!sp || !tp) return { ...edge, kind: edge.kind || 'forward' };
    return { ...edge, kind: geometricRouteKind(sp, tp) };
  });

  const sorted = [...classified].sort((a, b) => {
    const kindOrder = { forward: 0, same: 1, back: 2 };
    if (kindOrder[a.kind] !== kindOrder[b.kind]) {
      return kindOrder[a.kind] - kindOrder[b.kind];
    }
    const ax = positions.get(a.source)?.x ?? 0;
    const bx = positions.get(b.source)?.x ?? 0;
    if (ax !== bx) return ax - bx;
    return (positions.get(a.target)?.y ?? 0) - (positions.get(b.target)?.y ?? 0);
  });

  return sorted.map((edge) => {
    const sp = positions.get(edge.source);
    const tp = positions.get(edge.target);
    if (!sp || !tp) {
      return {
        source: edge.source,
        target: edge.target,
        kind: edge.kind,
        sourcePort: 'out',
        targetPort: 'in',
        router: { name: 'orth', args: { padding: 24 } },
        connector: ROUNDED_CONNECTOR,
        vertices: [],
        attrs: forwardAttrs,
      };
    }

    const primarySides = chooseSides(sp, tp);
    const obstacles = buildObstacleRects(positions, [edge.source, edge.target]);
    const attrs = edge.kind === 'back' ? backAttrs : forwardAttrs;
    const bounds = pairBounds(sp, tp);
    const sidePrefer = bounds.maxX + MIN_SEG;

    const planForSides = (outSide, inSide) => {
      const outMeta = peekStub(stubOutCount, edge.source, outSide);
      const inMeta = peekStub(stubInCount, edge.target, inSide);
      const exit = sideStub(sp, outSide, outMeta.stub, outMeta.along, 'out');
      const enter = sideStub(tp, inSide, inMeta.stub, inMeta.along, 'in');
      const outVert = outSide === 'top' || outSide === 'bottom';
      const midPrefer = outVert
        ? gutterLane(
            `vy:${keyPair(edge.source, edge.target)}:${Math.round((exit.y + enter.y) / 2)}`,
            (exit.y + enter.y) / 2
          )
        : gutterLane(
            `g:${keyPair(edge.source, edge.target)}:${Math.round((exit.x + enter.x) / 2)}`,
            (exit.x + enter.x) / 2
          );
      return {
        exit,
        enter,
        outSide,
        inSide,
        outMeta,
        inMeta,
        path: routeBetweenStubs(exit, enter, outSide, inSide, midPrefer),
      };
    };

    const primary = planForSides(primarySides.outSide, primarySides.inSide);

    const altSides =
      primarySides.outSide === 'top' || primarySides.outSide === 'bottom'
        ? {
            outSide: portX(tp) >= portX(sp) ? 'right' : 'left',
            inSide: portX(tp) >= portX(sp) ? 'left' : 'right',
          }
        : {
            outSide: portY(tp) >= portY(sp) ? 'bottom' : 'top',
            inSide: portY(tp) >= portY(sp) ? 'top' : 'bottom',
          };
    const alternate = planForSides(altSides.outSide, altSides.inSide);

    const ltr =
      primary.outSide === 'right' || primary.outSide === 'left'
        ? primary
        : alternate.outSide === 'right' || alternate.outSide === 'left'
          ? alternate
          : planForSides('right', 'left');
    const { laneY: underY } = takeLocalUnder(sp, tp);
    const { laneY: overY } = takeLocalOver(sp, tp);
    const candidates = [
      primary.path,
      alternate.path,
      underU(ltr.exit.x, ltr.exit.y, ltr.enter.x, ltr.enter.y, underY, sidePrefer),
      overU(ltr.exit.x, ltr.exit.y, ltr.enter.x, ltr.enter.y, overY, sidePrefer),
      sideC(ltr.exit.x, ltr.exit.y, ltr.enter.x, ltr.enter.y, sidePrefer),
    ];

    const chosenPath = pickClearPath(candidates, obstacles);
    let chosen = primary;
    if (chosenPath === alternate.path) chosen = alternate;
    else if (chosenPath !== primary.path) chosen = ltr;

    commitStub(stubOutCount, chosen.outMeta.key, chosen.outMeta.i);
    commitStub(stubInCount, chosen.inMeta.key, chosen.inMeta.i);

    return {
      source: edge.source,
      target: edge.target,
      kind: edge.kind,
      sourcePort: chosen.exit.port,
      targetPort: chosen.enter.port,
      outSide: chosen.outSide,
      inSide: chosen.inSide,
      router: { name: 'normal' },
      connector: ROUNDED_CONNECTOR,
      vertices: chosenPath,
      attrs,
    };
  });
}

/**
 * Layout nodes LTR by BFS rank and build non-overlapping edge plans.
 * @returns {{ positions: Map<string,{x:number,y:number}>, ranks: Map<string,number>, edges: object[] }}
 */
export function layoutSceneGraph(sceneGraph, rootId, nodeIds, { isDark = false } = {}) {
  const ids = nodeIds?.length ? nodeIds : Object.keys(sceneGraph || {});
  if (!ids.length) {
    return { positions: new Map(), ranks: new Map(), edges: [] };
  }

  const { outgoing, incoming } = buildAdjacency(sceneGraph, ids);
  const { ranks, orphans } = assignRanks(outgoing, ids, rootId);

  const byLevel = new Map();
  for (const [id, lv] of ranks) {
    if (!byLevel.has(lv)) byLevel.set(lv, []);
    byLevel.get(lv).push(id);
  }
  orderByBarycenter(byLevel, outgoing, incoming);

  const { positions } = placeNodes(byLevel, orphans, ids, ranks);
  const rawEdges = collectEdges(outgoing, ranks);
  const edges = planEdges(rawEdges, positions, { isDark });

  return { positions, ranks, edges };
}

/** Build an X6 edge config object from a layout plan. */
export function planToEdgeConfig(plan) {
  return {
    shape: STAGE_EDGE_SHAPEID,
    source: { cell: plan.source, port: plan.sourcePort || 'out' },
    target: { cell: plan.target, port: plan.targetPort || 'in' },
    router: plan.router,
    connector: plan.connector,
    vertices: plan.vertices || [],
    attrs: plan.attrs,
  };
}

/** Apply a layout plan onto an existing X6 edge. Returns true if geometry changed. */
export function applyEdgePlan(edge, plan) {
  if (!edge || !plan) return false;
  const prev = edge.getVertices() || [];
  const next = plan.vertices || [];
  const prevSrc = edge.getSource?.() || {};
  const prevTgt = edge.getTarget?.() || {};
  const nextSrcPort = plan.sourcePort || 'out';
  const nextTgtPort = plan.targetPort || 'in';
  const vertsChanged =
    prev.length !== next.length ||
    prev.some(
      (p, i) =>
        Math.abs((p.x || 0) - (next[i]?.x || 0)) > 0.5 ||
        Math.abs((p.y || 0) - (next[i]?.y || 0)) > 0.5
    );
  const portsChanged =
    prevSrc.port !== nextSrcPort || prevTgt.port !== nextTgtPort;
  edge.setSource({ cell: plan.source, port: nextSrcPort });
  edge.setTarget({ cell: plan.target, port: nextTgtPort });
  edge.setRouter(plan.router);
  edge.setConnector(plan.connector);
  edge.setVertices(next);
  if (plan.attrs) edge.setAttrs(plan.attrs);
  return vertsChanged || portsChanged;
}

/**
 * Re-route edges for the current node positions without moving nodes.
 * Useful when opening a scene that already has coords.
 */
export function routeEdgesForPositions(sceneGraph, rootId, nodeIds, positions, { isDark = false } = {}) {
  const ids = nodeIds?.length ? nodeIds : Object.keys(sceneGraph || {});
  const { outgoing } = buildAdjacency(sceneGraph, ids);
  const { ranks, orphans } = assignRanks(outgoing, ids, rootId);
  orphans.forEach((id) => {
    if (!ranks.has(id)) ranks.set(id, -1);
  });
  const rawEdges = collectEdges(outgoing, ranks);
  return { edges: planEdges(rawEdges, positions, { isDark }), ranks };
}

/** Keep only edge plans whose source\0target key is in `visibleKeys`. */
export function filterEdgePlans(plans, visibleKeys) {
  if (!visibleKeys) return plans;
  return plans.filter((p) => visibleKeys.has(`${p.source}\0${p.target}`));
}

export function edgeKey(source, target) {
  return `${source}\0${target}`;
}
