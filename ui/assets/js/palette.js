// Command palette (Cmd/Ctrl+K): search entities, run commands.
const commands = [
  { title: 'Go to Overview', url: '/' },
  { title: 'Go to Attention', url: '/attention' },
  { title: 'Go to Features', url: '/features' },
  { title: 'Go to Changes', url: '/changes' },
  { title: 'Go to Tests', url: '/tests' },
  { title: 'Go to Performance', url: '/performance' },
  { title: 'Go to Visual', url: '/visual' },
  { title: 'Go to ADRs', url: '/adrs' },
  { title: 'Show Failing Tests', url: '/tests?status=failing' },
  { title: 'Show Flaky Tests', url: '/tests?status=flaky' },
  { title: 'Show Benchmark Regressions', url: '/performance?state=regression' },
  { title: 'Show Budget Breaches', url: '/performance?state=breach' },
  { title: 'Show Visual Regressions', url: '/visual?status=regression' },
  { title: 'Show Claims Matrix', url: '/features?cols=claims' },
  { title: 'Show Broken Features', url: '/features?state=broken' },
  { title: 'Show Features Needing Attention', url: '/features?state=attention' },
  { title: 'Show Unknown Features', url: '/features?state=unknown' },
  { title: 'Show Recent Changes', url: '/changes' },
  { title: 'Refresh Project', action: 'rescan' },
  { title: 'Toggle Theme', action: 'theme' },
  { title: 'Copy Current ID', action: 'copy-id' },
  { title: 'Project Status', url: '/status' },
];

let cursor = 0;
let items = [];

function go(url) {
  const d = document.getElementById('palette');
  d?.close();
  if (window.htmx) htmx.ajax('GET', url, { target: '#main', swap: 'outerHTML show:window:top', pushUrl: true });
  else location.href = url;
}

function run(item) {
  if (item.url) return go(item.url);
  const d = document.getElementById('palette');
  d?.close();
  if (item.action === 'rescan') fetch('/api/v1/scan', { method: 'POST', headers: { 'X-Cockpit': '1' } });
  if (item.action === 'theme') document.querySelector('[data-theme-toggle]')?.click();
  if (item.action === 'copy-id') {
    const id = document.getElementById('main')?.dataset.entityId;
    if (id) navigator.clipboard.writeText(id);
  }
}

function render(groups) {
  const host = document.querySelector('[data-palette-results]');
  if (!host) return;
  items = [];
  host.innerHTML = '';
  for (const g of groups) {
    const h = document.createElement('div');
    h.className = 'palette__g';
    h.textContent = g.kind;
    host.appendChild(h);
    for (const it of g.hits) {
      const row = document.createElement('div');
      row.className = 'palette__it';
      row.innerHTML = `<span class="t"></span><span class="mono"></span><span class="sub"></span>`;
      row.querySelector('.t').textContent = it.title;
      row.querySelector('.mono').textContent = it.id || '';
      row.querySelector('.sub').textContent = it.subtitle || '';
      row.addEventListener('click', () => run(it));
      host.appendChild(row);
      items.push({ el: row, it });
    }
  }
  cursor = 0;
  paint();
}

function paint() {
  items.forEach((x, i) => x.el.classList.toggle('is-cursor', i === cursor));
  items[cursor]?.el.scrollIntoView({ block: 'nearest' });
}

let debounce = null;
async function query(q) {
  if (q.startsWith('>')) {
    const needle = q.slice(1).trim().toLowerCase();
    const hits = commands.filter((c) => c.title.toLowerCase().includes(needle));
    return render([{ kind: 'Commands', hits }]);
  }
  if (!q.trim()) {
    return render([{ kind: 'Commands', hits: commands.slice(0, 8) }]);
  }
  try {
    const res = await fetch(`/api/v1/search?q=${encodeURIComponent(q)}&limit=6`);
    const json = await res.json();
    const groups = json.data.groups || [];
    const cmdHits = commands.filter((c) => c.title.toLowerCase().includes(q.toLowerCase())).slice(0, 3);
    if (cmdHits.length) groups.push({ kind: 'Commands', hits: cmdHits });
    render(groups);
  } catch {
    render([]);
  }
}

export function initPalette() {
  const d = document.getElementById('palette');
  const input = d?.querySelector('[data-palette-input]');
  if (!d || !input) return;
  const open = () => { d.showModal(); input.value = ''; input.focus(); query(''); };
  document.addEventListener('keydown', (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') { e.preventDefault(); d.open ? d.close() : open(); }
  });
  document.addEventListener('cockpit:palette', open);
  document.addEventListener('click', (e) => { if (e.target.closest('[data-palette-open]')) open(); });
  input.addEventListener('input', () => { clearTimeout(debounce); debounce = setTimeout(() => query(input.value), 80); });
  input.addEventListener('keydown', (e) => {
    if (e.key === 'ArrowDown') { e.preventDefault(); cursor = Math.min(items.length - 1, cursor + 1); paint(); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); cursor = Math.max(0, cursor - 1); paint(); }
    else if (e.key === 'Enter') { e.preventDefault(); if (items[cursor]) run(items[cursor].it); }
  });
  d.addEventListener('click', (e) => { if (e.target === d) d.close(); });
}
