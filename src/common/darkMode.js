const STORAGE_KEY = 'slsb-darkmode';

export function readOsDarkMode() {
  try {
    if (typeof window !== 'undefined' && window.matchMedia) {
      return window.matchMedia('(prefers-color-scheme: dark)').matches;
    }
  } catch {
    /* ignore */
  }
  return false;
}

export function readStoredDarkMode() {
  try {
    if (typeof window !== 'undefined' && typeof window.__SLSB_DARK__ === 'boolean') {
      return window.__SLSB_DARK__;
    }
  } catch {
    /* ignore */
  }
  return readOsDarkMode();
}

export function writeStoredDarkMode(isDark) {
  try {
    localStorage.setItem(STORAGE_KEY, isDark ? '1' : '0');
    window.__SLSB_DARK__ = !!isDark;
  } catch {
    /* ignore quota / private mode */
  }
}

export function applyRootDarkClass(isDark) {
  const root = document.getElementById('root');
  if (!root) return;
  root.classList.toggle('dark-mode', isDark);
  root.classList.toggle('light-mode', !isDark);
  document.documentElement.style.colorScheme = isDark ? 'dark' : 'light';
  document.documentElement.style.background = isDark ? '#141414' : '#f5f5f5';
}
