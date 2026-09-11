// Compare viewer for visual checks: expected / actual / diff, side by side, overlay with swipe
// and opacity, blink. Fit / 100% / zoom / pan. Keys: 1 2 3 s o b f 0 + - arrows.

const MODES = ['expected', 'actual', 'diff', 'side', 'overlay', 'blink'];

function initViewer(root) {
  if (root.dataset.ready) return;
  root.dataset.ready = '1';
  const urls = { expected: root.dataset.expected, actual: root.dataset.actual, diff: root.dataset.diff };
  const stage = root.querySelector('[data-stage]');
  const paneA = root.querySelector('[data-pane="a"]');
  const paneB = root.querySelector('[data-pane="b"]');
  const imgA = paneA.querySelector('img');
  const imgB = paneB.querySelector('img');
  const swipe = root.querySelector('[data-swipe]');
  const opacity = root.querySelector('[data-opacity]');
  const zoomLabel = root.querySelector('[data-zoom-label]');
  let mode = root.dataset.mode || 'actual';
  let zoom = 0; // 0 = fit
  let pan = { x: 0, y: 0 };
  let swipePos = 0.5;
  let blinkTimer = null;
  let blinkState = false;

  function available(m) {
    if (m === 'expected') return !!urls.expected;
    if (m === 'actual') return !!urls.actual;
    if (m === 'diff') return !!urls.diff;
    return !!(urls.expected && urls.actual);
  }

  function applyZoom() {
    const scale = zoom === 0 ? '' : `scale(${zoom})`;
    for (const img of [imgA, imgB]) {
      img.style.transform = `translate(${pan.x}px, ${pan.y}px) ${scale}`;
      img.classList.toggle('is-fit', zoom === 0);
    }
    zoomLabel.textContent = zoom === 0 ? 'fit' : `${Math.round(zoom * 100)}%`;
  }

  function setMode(m) {
    if (!available(m)) return;
    mode = m;
    root.dataset.mode = m;
    if (blinkTimer) { clearInterval(blinkTimer); blinkTimer = null; }
    root.querySelectorAll('[data-mode-btn]').forEach((b) => b.classList.toggle('is-on', b.dataset.modeBtn === m));
    swipe.hidden = m !== 'overlay';
    paneB.style.opacity = '';
    paneB.style.clipPath = '';
    paneA.hidden = false; paneB.hidden = false;
    if (m === 'expected' || m === 'actual' || m === 'diff') {
      imgA.src = urls[m]; paneB.hidden = true;
    } else if (m === 'side') {
      imgA.src = urls.expected; imgB.src = urls.actual;
    } else if (m === 'overlay') {
      imgA.src = urls.expected; imgB.src = urls.actual;
      paneB.style.opacity = String((opacity ? opacity.value : 50) / 100);
      updateSwipe();
    } else if (m === 'blink') {
      imgA.src = urls.expected; imgB.src = urls.actual; paneB.hidden = true;
      blinkTimer = setInterval(() => {
        blinkState = !blinkState;
        imgA.src = blinkState ? urls.actual : urls.expected;
        imgA.alt = blinkState ? 'actual' : 'expected';
      }, 600);
    }
    root.querySelectorAll('[data-overlay-only]').forEach((el) => { el.hidden = m !== 'overlay'; });
    stage.classList.toggle('is-side', m === 'side');
    stage.classList.toggle('is-overlay', m === 'overlay');
    try { const u = new URL(location.href); u.searchParams.set('mode', m); history.replaceState(null, '', u); } catch {}
  }

  function updateSwipe() {
    const pct = Math.round(swipePos * 100);
    paneB.style.clipPath = `inset(0 0 0 ${pct}%)`;
    swipe.style.left = `${pct}%`;
  }

  root.addEventListener('click', (e) => {
    const b = e.target.closest('[data-mode-btn]');
    if (b) { setMode(b.dataset.modeBtn); return; }
    const z = e.target.closest('[data-zoom]');
    if (z) {
      const k = z.dataset.zoom;
      if (k === 'fit') { zoom = 0; pan = { x: 0, y: 0 }; }
      else if (k === '100') { zoom = 1; pan = { x: 0, y: 0 }; }
      else if (k === 'in') { zoom = Math.min(8, (zoom || 1) * 1.25); }
      else if (k === 'out') { zoom = Math.max(0.1, (zoom || 1) / 1.25); }
      applyZoom();
    }
  });
  if (opacity) opacity.addEventListener('input', () => { if (mode === 'overlay') paneB.style.opacity = String(opacity.value / 100); });

  // Swipe handle + pan.
  let dragging = null;
  stage.addEventListener('pointerdown', (e) => {
    if (mode === 'overlay' && Math.abs(e.offsetX / stage.clientWidth - swipePos) < 0.03) dragging = 'swipe';
    else if (zoom !== 0) dragging = { x: e.clientX - pan.x, y: e.clientY - pan.y };
    else return;
    stage.setPointerCapture(e.pointerId);
  });
  stage.addEventListener('pointermove', (e) => {
    if (!dragging) return;
    if (dragging === 'swipe') { swipePos = Math.min(1, Math.max(0, e.offsetX / stage.clientWidth)); updateSwipe(); }
    else { pan = { x: e.clientX - dragging.x, y: e.clientY - dragging.y }; applyZoom(); }
  });
  stage.addEventListener('pointerup', () => { dragging = null; });
  stage.addEventListener('wheel', (e) => {
    if (!e.ctrlKey && !e.metaKey) return;
    e.preventDefault();
    zoom = Math.min(8, Math.max(0.1, (zoom || 1) * (e.deltaY < 0 ? 1.1 : 0.9)));
    applyZoom();
  }, { passive: false });

  stage.addEventListener('keydown', (e) => {
    const k = e.key;
    if (k === '1') setMode('expected');
    else if (k === '2') setMode('actual');
    else if (k === '3') setMode('diff');
    else if (k === 's') setMode('side');
    else if (k === 'o') setMode('overlay');
    else if (k === 'b') setMode('blink');
    else if (k === 'f') { zoom = 0; pan = { x: 0, y: 0 }; applyZoom(); }
    else if (k === '0') { zoom = 1; pan = { x: 0, y: 0 }; applyZoom(); }
    else if (k === '+' || k === '=') { zoom = Math.min(8, (zoom || 1) * 1.25); applyZoom(); }
    else if (k === '-') { zoom = Math.max(0.1, (zoom || 1) / 1.25); applyZoom(); }
    else if (k === 'ArrowLeft' && mode === 'overlay') { swipePos = Math.max(0, swipePos - 0.02); updateSwipe(); }
    else if (k === 'ArrowRight' && mode === 'overlay') { swipePos = Math.min(1, swipePos + 0.02); updateSwipe(); }
    else if (k === 'ArrowLeft' || k === 'ArrowRight') {
      const a = document.querySelector(k === 'ArrowLeft' ? '.pager a[aria-label="previous"]' : '.pager a[aria-label="next"]');
      if (a) a.click();
    } else return;
    e.preventDefault();
  });

  setMode(MODES.includes(mode) && available(mode) ? mode : (urls.actual ? 'actual' : urls.expected ? 'expected' : 'diff'));
  applyZoom();
}

export function initCompare() {
  const mount = () => document.querySelectorAll('[data-viewer]').forEach(initViewer);
  mount();
  document.addEventListener('cockpit:page', mount);
  document.addEventListener('htmx:afterSwap', mount);
}
