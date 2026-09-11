// SSE client: one EventSource per tab; coalesces refreshes; never moves content under the user.
import { toast } from './app.js';

let queued = new Set();
let timer = null;
let lastSeq = 0;

function dot(state) {
  const d = document.getElementById('live-dot');
  if (d) d.className = `live-dot ${state}`;
}

function refresh(regions) {
  for (const r of regions) queued.add(r);
  clearTimeout(timer);
  timer = setTimeout(flush, 300);
}

function flush() {
  if (document.hidden || document.activeElement?.matches('input, textarea, select') || document.querySelector('dialog[open]')) {
    timer = setTimeout(flush, 1000);
    return;
  }
  const regions = [...queued];
  queued = new Set();
  const all = regions.includes('*');
  document.querySelectorAll('[data-live]').forEach((el) => {
    const mine = el.dataset.live.split(',').map((s) => s.trim());
    if (all || mine.some((m) => regions.includes(m))) {
      if (el.getAttribute('hx-get')) {
        htmx.trigger(el, 'cockpit:refresh');
      } else {
        // Regions without their own fragment endpoint refresh the page body in place.
        htmx.ajax('GET', location.pathname + location.search, { target: '#main', swap: 'morph:outerHTML', pushUrl: false });
      }
      el.classList.add('is-updated');
      setTimeout(() => el.classList.remove('is-updated'), 1200);
    }
  });
  const updated = document.getElementById('updated-at');
  if (updated) {
    const t = updated.querySelector('time');
    const now = new Date().toISOString();
    if (t) { t.setAttribute('datetime', now); t.textContent = 'just now'; } else updated.textContent = 'Updated just now';
  }
}

export function initLive() {
  if (!('EventSource' in window)) return;
  let es;
  let failures = 0;
  const connect = () => {
    es = new EventSource('/events');
    es.addEventListener('hello', (e) => {
      failures = 0;
      dot('is-live');
      try {
        const d = JSON.parse(e.data);
        if (lastSeq && d.seq > lastSeq) refresh(['*']);
        lastSeq = d.seq;
      } catch {}
    });
    es.addEventListener('project.scan.started', () => dot('is-scanning'));
    es.addEventListener('project.scan.completed', (e) => {
      dot('is-live');
      try { const d = JSON.parse(e.data); lastSeq = Number(e.lastEventId) || lastSeq; if (document.querySelector('[data-scan-progress]')) { htmx.ajax('GET', '/', { target: '#main', swap: 'outerHTML', pushUrl: false }); return; } } catch {}
      refresh(['*']);
    });
    es.addEventListener('feature.health.changed', (e) => {
      try {
        const d = JSON.parse(e.data);
        const ann = document.getElementById('a11y-announcer');
        if (ann) ann.textContent = `${d.title}: ${d.from || 'new'} → ${d.to}`;
        if (d.from) toast(`${d.title}: ${d.from} → ${d.to}`);
      } catch {}
      refresh(['attention', 'matrix', 'features', 'summary']);
    });
    es.addEventListener('attention.created', () => refresh(['attention']));
    es.addEventListener('attention.resolved', () => refresh(['attention']));
    es.addEventListener('state.resync', () => refresh(['*']));
    es.addEventListener('dev.reload', () => location.reload());
    es.onerror = () => {
      dot('is-off');
      failures += 1;
      if (failures > 5) { es.close(); setTimeout(connect, Math.min(30000, 1000 * 2 ** failures)); }
    };
  };
  connect();
}
