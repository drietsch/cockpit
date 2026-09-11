// Charts: ECharts is loaded on demand, only on pages that carry a [data-chart] element.
// Options are read from an embedded <script type="application/json"> so no inline JS runs.

let loading = null;
const live = new Map();

function token(name) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

function theme() {
  return {
    text: token('--text-2'), muted: token('--text-3'), border: token('--border'), grid: token('--border-soft'),
    line: token('--info'), ok: token('--healthy'), warn: token('--attention'), bad: token('--broken'), unk: token('--unknown'),
    surface: token('--surface-raised'), font: token('--font-ui'), mono: token('--font-mono'),
  };
}

function loadECharts() {
  if (window.echarts) return Promise.resolve(window.echarts);
  if (loading) return loading;
  loading = new Promise((resolve, reject) => {
    const s = document.createElement('script');
    s.src = '/assets/vendor/echarts.common.min.js';
    s.onload = () => resolve(window.echarts);
    s.onerror = () => reject(new Error('echarts failed to load'));
    document.head.appendChild(s);
  });
  return loading;
}

function fmtValue(v, unit) {
  if (v == null || Number.isNaN(v)) return '—';
  const a = Math.abs(v);
  const n = a >= 100 ? v.toFixed(0) : a >= 10 ? v.toFixed(1) : v.toFixed(2);
  return unit ? `${n} ${unit}` : n;
}

function fmtDate(ms) {
  const d = new Date(ms);
  return d.toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' }) + ' ' + d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
}

function lineHistory(data, t) {
  const pts = data.points || [];
  const comparable = pts.filter((p) => p.comparable !== false);
  const other = pts.filter((p) => p.comparable === false);
  const toXY = (p) => ({ value: [p.ms, p.v], meta: p });
  const markLines = [];
  if (data.budget != null) markLines.push({ yAxis: data.budget, name: 'budget', lineStyle: { color: t.bad, type: 'dashed' }, label: { formatter: 'budget', color: t.bad, position: 'insideEndTop', fontSize: 10 } });
  if (data.baseline != null) markLines.push({ yAxis: data.baseline, name: 'baseline', lineStyle: { color: t.muted, type: 'dotted' }, label: { formatter: 'baseline', color: t.muted, position: 'insideEndBottom', fontSize: 10 } });
  for (const tag of data.tags || []) {
    if (pts.length && tag.ms >= pts[0].ms - 1 && tag.ms <= pts[pts.length - 1].ms + 1) {
      markLines.push({ xAxis: tag.ms, lineStyle: { color: t.border, type: 'solid' }, label: { formatter: tag.tag, color: t.muted, fontSize: 10, position: 'insideStartTop' } });
    }
  }
  const markPoints = [];
  const markAreas = [];
  if (data.origin_ms != null) {
    const o = comparable.find((p) => p.ms === data.origin_ms);
    if (o) markPoints.push({ coord: [o.ms, o.v], symbol: 'pin', symbolSize: 26, itemStyle: { color: t.warn }, label: { show: false } });
    markAreas.push([{ xAxis: data.origin_ms, itemStyle: { color: t.warn, opacity: 0.06 } }, { xAxis: pts.length ? pts[pts.length - 1].ms : data.origin_ms }]);
  }
  const last = comparable[comparable.length - 1];
  const series = [{
    type: 'line', name: 'value', data: comparable.map(toXY), showSymbol: true, symbolSize: 4,
    lineStyle: { color: t.line, width: 1.5 }, itemStyle: { color: t.line },
    emphasis: { focus: 'series', itemStyle: { borderWidth: 2 } },
    markLine: { silent: true, symbol: 'none', data: markLines, animation: false },
    markPoint: { data: markPoints, animation: false },
    markArea: { silent: true, data: markAreas, animation: false },
    sampling: comparable.length > 400 ? 'lttb' : undefined,
  }];
  if (last) series.push({ type: 'scatter', name: 'current', data: [toXY(last)], symbolSize: 9, itemStyle: { color: t.line, borderColor: t.surface, borderWidth: 2 }, z: 5 });
  if (other.length) series.push({ type: 'scatter', name: 'other environment', data: other.map(toXY), symbol: 'diamond', symbolSize: 6, itemStyle: { color: t.unk, opacity: 0.8 } });
  const unit = data.unit || '';
  return {
    animation: false,
    grid: { left: 56, right: 24, top: 20, bottom: pts.length > 40 ? 48 : 28, containLabel: false },
    textStyle: { fontFamily: t.font, color: t.text },
    tooltip: {
      trigger: 'item', backgroundColor: t.surface, borderColor: t.border, textStyle: { color: t.text, fontSize: 12 },
      formatter: (p) => {
        const m = p.data && p.data.meta; if (!m) return '';
        const bits = [`<b>${m.text || fmtValue(m.v, unit)}</b>`, fmtDate(m.ms)];
        if (m.sha) bits.push(`<span style="font-family:${t.mono}">${m.sha}</span>${m.dirty ? ' · dirty' : ''}${m.branch ? ' · ' + m.branch : ''}`);
        if (m.env) bits.push(m.env);
        if (m.p50 != null) bits.push(`p50 ${fmtValue(m.p50, unit)} · p95 ${fmtValue(m.p95, unit)}`);
        if (m.accepted) bits.push(`accepted: ${m.accepted}`);
        if (m.comparable === false) bits.push('different environment — not compared');
        return bits.join('<br>');
      },
    },
    xAxis: { type: 'time', axisLine: { lineStyle: { color: t.border } }, axisLabel: { color: t.muted, fontSize: 11 }, splitLine: { show: false } },
    yAxis: { type: 'value', scale: true, axisLabel: { color: t.muted, fontSize: 11, formatter: (v) => fmtValue(v, '') }, splitLine: { lineStyle: { color: t.grid } }, name: unit, nameTextStyle: { color: t.muted, fontSize: 10, align: 'right' } },
    dataZoom: pts.length > 40 ? [{ type: 'slider', height: 16, bottom: 6, borderColor: t.border, backgroundColor: 'transparent', fillerColor: 'rgba(110,168,254,.12)', handleSize: 12, textStyle: { color: t.muted, fontSize: 10 } }] : undefined,
    series,
  };
}

function hbar(data, t) {
  const rows = (data || []).slice().reverse();
  return {
    animation: false,
    grid: { left: 8, right: 60, top: 4, bottom: 4, containLabel: true },
    textStyle: { fontFamily: t.font, color: t.text },
    tooltip: { trigger: 'item', backgroundColor: t.surface, borderColor: t.border, textStyle: { color: t.text, fontSize: 12 }, formatter: (p) => `${p.name}: ${p.value > 0 ? '+' : ''}${p.value.toFixed(1)}%` },
    xAxis: { type: 'value', axisLabel: { color: t.muted, fontSize: 10, formatter: (v) => `${v > 0 ? '+' : ''}${v}%` }, splitLine: { lineStyle: { color: t.grid } } },
    yAxis: { type: 'category', data: rows.map((r) => r.label), axisLabel: { color: t.text, fontSize: 11, fontFamily: t.mono, width: 260, overflow: 'truncate' }, axisTick: { show: false }, axisLine: { show: false } },
    series: [{ type: 'bar', data: rows.map((r) => ({ value: Math.abs(r.pct || 0) * (r.pct < 0 ? -1 : 1), id: r.id })), barMaxWidth: 12, itemStyle: { color: t.warn, borderRadius: 2 }, label: { show: true, position: 'right', color: t.muted, fontSize: 10, formatter: (p) => `${p.value > 0 ? '+' : ''}${p.value.toFixed(1)}%` } }],
  };
}

function durationTrend(data, t) {
  const pts = data.points || [];
  return {
    animation: false,
    grid: { left: 48, right: 16, top: 12, bottom: 24 },
    textStyle: { fontFamily: t.font, color: t.text },
    tooltip: { trigger: 'axis', backgroundColor: t.surface, borderColor: t.border, textStyle: { color: t.text, fontSize: 12 } },
    xAxis: { type: 'time', axisLabel: { color: t.muted, fontSize: 10 }, axisLine: { lineStyle: { color: t.border } }, splitLine: { show: false } },
    yAxis: { type: 'value', axisLabel: { color: t.muted, fontSize: 10, formatter: (v) => `${v} ms` }, splitLine: { lineStyle: { color: t.grid } } },
    series: [{ type: 'line', data: pts.map((p) => [p.ms, p.v]), lineStyle: { color: t.line, width: 1.5 }, itemStyle: { color: t.line }, symbolSize: 4 }],
  };
}

const builders = { lineHistory, hbar, durationTrend };

function mount(el) {
  const kind = el.dataset.chart;
  const builder = builders[kind];
  const src = el.querySelector('script[type="application/json"]');
  if (!builder || !src) return;
  let data;
  try { data = JSON.parse(src.textContent); } catch { return; }
  const h = parseInt(el.dataset.chartHeight || '240', 10);
  let host = el.querySelector('.chart__host');
  if (!host) { host = document.createElement('div'); host.className = 'chart__host'; host.style.height = `${h}px`; el.appendChild(host); }
  loadECharts().then((echarts) => {
    if (!document.contains(el)) return;
    const chart = echarts.init(host, null, { renderer: 'canvas' });
    chart.setOption(builder(data, theme()));
    chart.on('click', (p) => {
      const m = p.data && p.data.meta;
      if (m && m.sha_full) {
        const url = `/commits/${encodeURIComponent(m.sha_full)}${m.repo ? '?repo=' + encodeURIComponent(m.repo) : ''}`;
        if (window.htmx) htmx.ajax('GET', url, { target: '#main', swap: 'outerHTML show:window:top', pushUrl: true }); else location.href = url;
      } else if (p.data && p.data.id) {
        const url = `/performance/${encodeURIComponent(p.data.id)}`;
        if (window.htmx) htmx.ajax('GET', url, { target: '#main', swap: 'outerHTML show:window:top', pushUrl: true }); else location.href = url;
      }
    });
    const ro = new ResizeObserver(() => chart.resize());
    ro.observe(host);
    live.set(el, { chart, ro });
  }).catch(() => {});
}

export function mountCharts(root = document) {
  root.querySelectorAll('[data-chart]').forEach((el) => { if (!live.has(el)) mount(el); });
}

export function disposeCharts() {
  for (const [el, { chart, ro }] of live) {
    if (!document.contains(el)) { ro.disconnect(); chart.dispose(); live.delete(el); }
  }
}

export function initCharts() {
  mountCharts();
  document.addEventListener('cockpit:page', () => { disposeCharts(); mountCharts(); });
  document.addEventListener('htmx:afterSwap', () => { disposeCharts(); mountCharts(); });
  document.addEventListener('cockpit:theme', () => {
    for (const [el, { chart, ro }] of live) { ro.disconnect(); chart.dispose(); live.delete(el); mount(el); }
  });
}
