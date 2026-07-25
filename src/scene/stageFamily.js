/**
 * Pose-family helpers for clustered graph layout / filtering.
 * Names look like: "Lovemaking: Reverse Embrace Idle|pb:1|ds:1"
 * or transitions: "Lovemaking: Go to Squatting Handjob|pb:1|ds:1"
 */

const MULTI_WORD_FAMILIES = [
  ['Reverse', 'Embrace'],
  ['Mating', 'Press'],
  ['Standing', 'Embrace'],
];

const SINGLE_WORD_FAMILIES = new Set([
  'Squatting',
  'Kneeling',
  'Standing',
  'Sitting',
  'Straddling',
  'Missionary',
  'Cowgirl',
  'Cuddling',
  'Devour',
  'Lying',
  'Prone',
  'Doggy',
]);

/** Abbreviation tokens used in "Go to RE …" style transition names. */
const FAMILY_ALIASES = {
  RE: 'Reverse Embrace',
  MP: 'Mating Press',
  SE: 'Standing Embrace',
};

/**
 * Strip pack prefix and playback tags from a stage display name.
 * @param {string} name
 * @returns {string}
 */
export function cleanStageName(name) {
  let n = String(name || '');
  n = n.replace(/^[^:]+:\s*/, ''); // "Lovemaking: " or similar pack prefix
  n = n.replace(/\|pb:.*$/i, '');
  return n.trim();
}

/**
 * Resolve a pose family label from a cleaned stage name (no pack/pb tags).
 * @param {string} cleaned
 * @returns {string}
 */
export function familyFromCleanName(cleaned) {
  const n = String(cleaned || '').trim();
  if (!n) return 'Other';

  const goTo = n.match(/^Go to\s+(.+)$/i);
  if (goTo) {
    return familyFromCleanName(goTo[1]);
  }

  const parts = n.replace(/-/g, ' ').split(/\s+/).filter(Boolean);
  if (!parts.length) return 'Other';

  const alias = FAMILY_ALIASES[parts[0].toUpperCase()];
  if (alias) return alias;

  for (const words of MULTI_WORD_FAMILIES) {
    if (
      parts.length >= words.length &&
      words.every((w, i) => parts[i].toLowerCase() === w.toLowerCase())
    ) {
      return words.join(' ');
    }
  }

  // "Reverse Foo" without Embrace → still Reverse Embrace if second token looks related
  if (parts[0] === 'Reverse' && parts[1] && parts[1] !== 'Embrace') {
    return 'Reverse Embrace';
  }

  if (SINGLE_WORD_FAMILIES.has(parts[0])) {
    return parts[0];
  }

  return parts[0] || 'Other';
}

/**
 * @param {string} stageName
 * @returns {string} family label
 */
export function poseFamily(stageName) {
  return familyFromCleanName(cleanStageName(stageName));
}

/**
 * True when the stage looks like a short "Go to …" transition node.
 * @param {string} stageName
 */
export function isTransitionStage(stageName) {
  return /^Go to\s+/i.test(cleanStageName(stageName));
}

/**
 * Prefer idle / hub naming when ranking candidates.
 * @param {string} stageName
 */
export function isHubName(stageName) {
  const n = cleanStageName(stageName);
  return /\bIdle\b/i.test(n) || /\bEmbrace\b/i.test(n) && !isTransitionStage(stageName);
}

/**
 * Map nodeId → family using a name getter.
 * @param {string[]} nodeIds
 * @param {(id: string) => string} getName
 * @returns {Map<string, string>}
 */
export function buildFamilyMap(nodeIds, getName) {
  const map = new Map();
  for (const id of nodeIds) {
    map.set(id, poseFamily(getName(id) || id));
  }
  return map;
}

export const LARGE_SCENE_NODE_THRESHOLD = 40;
