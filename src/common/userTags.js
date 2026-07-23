const STORAGE_KEY = 'slsb.userTags';

function tagKey(tag) {
  return String(tag).toLowerCase().replace(/\s+/g, '');
}

export function loadUserTags() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    const parsed = raw ? JSON.parse(raw) : [];
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((t) => typeof t === 'string' && t.trim())
      .map((t) => t.trim());
  } catch {
    return [];
  }
}

/** Persist tags that are not in the built-in preset lists. */
export function rememberUserTags(candidates, presets = []) {
  const presetKeys = new Set(presets.map(tagKey));
  const byKey = new Map(loadUserTags().map((t) => [tagKey(t), t]));
  let changed = false;
  for (const tag of candidates) {
    const trimmed = String(tag ?? '').trim();
    if (!trimmed) continue;
    const key = tagKey(trimmed);
    if (presetKeys.has(key) || byKey.has(key)) continue;
    byKey.set(key, trimmed);
    changed = true;
  }
  if (!changed) return [...byKey.values()];
  const next = [...byKey.values()].sort((a, b) => a.localeCompare(b));
  localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
  return next;
}
