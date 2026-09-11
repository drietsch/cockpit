// Keep relative times honest without a reload. Mirrors time::ago in Rust.
export function ago(iso, now = Date.now()) {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return '';
  const s = Math.floor((now - t) / 1000);
  if (s < 0) return 'in the future';
  if (s <= 44) return 'just now';
  if (s <= 89) return '1 min ago';
  if (s < 3600) return `${Math.floor((s + 30) / 60)} min ago`;
  if (s < 86400) return `${Math.floor((s + 1800) / 3600)} h ago`;
  if (s < 1209600) return `${Math.floor((s + 43200) / 86400)} d ago`;
  if (s < 4838400) return `${Math.floor((s + 302400) / 604800)} w ago`;
  const d = new Date(t);
  const sameYear = d.getUTCFullYear() === new Date(now).getUTCFullYear();
  const opts = sameYear ? { month: 'short', day: 'numeric' } : { month: 'short', day: 'numeric', year: 'numeric' };
  return d.toLocaleDateString('en-US', { ...opts, timeZone: 'UTC' });
}

export function initRelTime() {
  const tick = () => {
    document.querySelectorAll('time[data-rel]').forEach((el) => {
      const v = ago(el.getAttribute('datetime'));
      if (v && el.textContent !== v) el.textContent = v;
    });
  };
  setInterval(tick, 30000);
  document.addEventListener('cockpit:page', tick);
}
