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

function saveUserTags(tags) {
  const next = [...tags].sort((a, b) => a.localeCompare(b));
  localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
  return next;
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
  return saveUserTags([...byKey.values()]);
}

/** Remove a saved custom tag from localStorage. */
export function removeUserTag(tag) {
  const key = tagKey(tag);
  const next = loadUserTags().filter((t) => tagKey(t) !== key);
  return saveUserTags(next);
}

/**
 * Rename a saved custom tag. Returns the updated list, or null if the new
 * name is empty / collides with a preset or another saved tag.
 */
export function renameUserTag(oldTag, newTag, presets = []) {
  const oldKey = tagKey(oldTag);
  const trimmed = String(newTag ?? '').trim();
  if (!trimmed) return null;

  const newKey = tagKey(trimmed);
  const presetKeys = new Set(presets.map(tagKey));
  if (presetKeys.has(newKey)) return null;

  const byKey = new Map(loadUserTags().map((t) => [tagKey(t), t]));
  if (!byKey.has(oldKey)) return null;
  if (newKey !== oldKey && byKey.has(newKey)) return null;

  byKey.delete(oldKey);
  byKey.set(newKey, trimmed);
  return saveUserTags([...byKey.values()]);
}
