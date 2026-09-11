// Theme toggle: the server renders data-theme from a cookie so there is no flash.
export function initTheme() {
  document.addEventListener('click', (e) => {
    const b = e.target.closest('[data-theme-toggle]');
    if (!b) return;
    const root = document.documentElement;
    const next = root.dataset.theme === 'light' ? 'dark' : 'light';
    root.dataset.theme = next;
    document.cookie = `cockpit_theme=${next}; path=/; max-age=31536000; SameSite=Lax`;
    document.dispatchEvent(new CustomEvent('cockpit:theme', { detail: next }));
  });
}
