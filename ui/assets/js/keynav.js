// Keyboard: `/` search, `g` chords, j/k row cursor, Enter opens, Esc closes, `?` help.
const routes = { o: '/', a: '/attention', f: '/features', c: '/changes', t: '/tests', p: '/performance', v: '/visual' };
let chord = null;
let chordTimer = null;

function typing() {
  const a = document.activeElement;
  return a && (a.matches('input, textarea, select, [contenteditable]') || a.closest('dialog[open]'));
}

function rows() {
  return [...document.querySelectorAll('[data-keynav] tr[data-href]')];
}

function moveCursor(delta) {
  const rs = rows();
  if (!rs.length) return;
  let i = rs.findIndex((r) => r.classList.contains('is-cursor'));
  rs.forEach((r) => r.classList.remove('is-cursor'));
  i = i < 0 ? (delta > 0 ? 0 : rs.length - 1) : Math.max(0, Math.min(rs.length - 1, i + delta));
  rs[i].classList.add('is-cursor');
  rs[i].scrollIntoView({ block: 'nearest' });
}

function go(url) {
  if (window.htmx) htmx.ajax('GET', url, { target: '#main', swap: 'outerHTML show:window:top', pushUrl: true });
  else location.href = url;
}

export function initKeynav() {
  document.addEventListener('keydown', (e) => {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    if (typing()) {
      if (e.key === 'Escape') document.activeElement.blur();
      return;
    }
    if (chord === 'g') {
      chord = null;
      clearTimeout(chordTimer);
      if (routes[e.key]) { e.preventDefault(); go(routes[e.key]); }
      return;
    }
    switch (e.key) {
      case 'g':
        chord = 'g';
        chordTimer = setTimeout(() => { chord = null; }, 800);
        break;
      case '/': {
        e.preventDefault();
        const f = document.querySelector('[data-page-filter]');
        if (f) f.focus(); else document.dispatchEvent(new CustomEvent('cockpit:palette'));
        break;
      }
      case 'j': e.preventDefault(); moveCursor(1); break;
      case 'k': e.preventDefault(); moveCursor(-1); break;
      case 'Enter': {
        const cur = document.querySelector('[data-keynav] tr.is-cursor');
        if (cur) { e.preventDefault(); go(cur.dataset.href); }
        break;
      }
      case '?': {
        e.preventDefault();
        const d = document.getElementById('shortcuts');
        if (d) d.showModal();
        break;
      }
      case 'Escape': {
        const open = document.querySelector('dialog[open]');
        if (open) open.close();
        break;
      }
      default:
    }
  });
  // Re-apply the cursor after a morph by stable id.
  document.addEventListener('htmx:beforeSwap', () => {
    const cur = document.querySelector('[data-keynav] tr.is-cursor');
    if (cur) document.body.dataset.cursorId = cur.dataset.id || '';
  });
  document.addEventListener('htmx:afterSettle', () => {
    const id = document.body.dataset.cursorId;
    if (!id) return;
    const r = document.querySelector(`[data-keynav] tr[data-id="${CSS.escape(id)}"]`);
    if (r) r.classList.add('is-cursor');
  });
}
