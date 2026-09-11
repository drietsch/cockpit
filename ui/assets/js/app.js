// Cockpit front-end entry: htmx configuration and the small behaviours the server cannot own.
import { initLive } from './live.js';
import { initTheme } from './theme.js';
import { initRelTime } from './reltime.js';
import { initKeynav } from './keynav.js';
import { initPalette } from './palette.js';

function configureHtmx() {
  if (!window.htmx) { setTimeout(configureHtmx, 20); return; }
  htmx.config.globalViewTransitions = false;
  htmx.config.scrollBehavior = 'instant';
  htmx.config.historyCacheSize = 0;
  htmx.config.refreshOnHistoryMiss = false;
  htmx.config.responseHandling = [
    { code: '204', swap: false },
    { code: '[23]..', swap: true },
    { code: '[45]..', swap: true, error: true },
  ];
  document.body.addEventListener('htmx:afterSettle', (e) => {
    if (e.target && e.target.id === 'main') onPage();
  });
  document.body.addEventListener('htmx:responseError', (e) => {
    toast(`Request failed (${e.detail.xhr.status})`);
  });
}

export function toast(text, ms = 2200) {
  const host = document.getElementById('toasts');
  if (!host) return;
  const el = document.createElement('div');
  el.className = 'toast';
  el.textContent = text;
  host.appendChild(el);
  setTimeout(() => el.remove(), ms);
}

function onPage() {
  const main = document.getElementById('main');
  if (main) main.focus({ preventScroll: true });
  const ann = document.getElementById('a11y-announcer');
  if (ann) ann.textContent = document.title;
  if (location.hash) {
    const t = document.getElementById(location.hash.slice(1));
    if (t) t.scrollIntoView();
  }
  document.dispatchEvent(new CustomEvent('cockpit:page'));
}

function initRowLinks() {
  document.addEventListener('click', (e) => {
    const row = e.target.closest('tr[data-href]');
    if (!row || e.target.closest('a, button, input, select, label')) return;
    if (e.metaKey || e.ctrlKey) { window.open(row.dataset.href, '_blank'); return; }
    if (window.htmx) htmx.ajax('GET', row.dataset.href, { target: '#main', swap: 'outerHTML show:window:top', pushUrl: true });
    else location.href = row.dataset.href;
  });
}

function initCopy() {
  document.addEventListener('click', async (e) => {
    const b = e.target.closest('[data-copy]');
    if (!b) return;
    e.preventDefault();
    try { await navigator.clipboard.writeText(b.dataset.copy); toast(`Copied ${b.dataset.copy}`); } catch { toast('Copy failed'); }
  });
}

function initBanner() {
  const b = document.querySelector('[data-banner]');
  if (!b) return;
  const key = 'cockpit.banner.dismissed';
  try { if (localStorage.getItem(key) === b.dataset.banner) b.hidden = true; } catch {}
  b.querySelector('[data-banner-dismiss]')?.addEventListener('click', () => {
    b.hidden = true;
    try { localStorage.setItem(key, b.dataset.banner); } catch {}
  });
}

function initRescan() {
  document.addEventListener('click', async (e) => {
    const b = e.target.closest('[data-rescan]');
    if (!b) return;
    b.disabled = true;
    try { await fetch('/api/v1/scan', { method: 'POST', headers: { 'X-Cockpit': '1' } }); toast('Rescan requested'); } catch { toast('Rescan failed'); }
    setTimeout(() => { b.disabled = false; }, 2000);
  });
}

configureHtmx();
initTheme();
initRelTime();
initKeynav();
initPalette();
initLive();
initRowLinks();
initCopy();
initBanner();
initRescan();
