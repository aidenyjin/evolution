// Panels: settings (generated from the parameter registry), discoveries
// log, graphs, toasts.

import { Chart, fmtTime, fmtNum } from './charts.js';

export function toast(text, kind = '') {
  const el = document.createElement('div');
  el.className = `toast ${kind}`;
  el.textContent = text;
  const box = document.getElementById('toasts');
  box.appendChild(el);
  while (box.children.length > 4) box.firstChild.remove();
  setTimeout(() => { el.style.transition = 'opacity .4s'; el.style.opacity = '0'; setTimeout(() => el.remove(), 450); }, kind === 'discovery' ? 6500 : 3500);
}

const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));

function fmtVal(v) {
  if (v === 0) return '0';
  const a = Math.abs(v);
  if (a >= 1000) return v.toFixed(0);
  if (a >= 100) return v.toFixed(1);
  if (a >= 1) return Number(v.toPrecision(4)).toString();
  return Number(v.toPrecision(3)).toString();
}

export const PRESETS = [
  { name: 'Early Earth (defaults)', reset: true, set: {} },
  { name: 'After the Great Oxidation (O₂ in the air)', set: { atm_o2: 1.2 } },
  { name: 'Day and night', set: { day_night: 1 } },
  { name: 'Fast evolution (×3 mutation)', set: { mut_point: 0.006, mut_indel: 0.0012, mut_dup: 0.09, mut_del: 0.06, mut_new_gene: 0.09, mut_promoter: 0.09 } },
  { name: 'Rich vents', set: { vent_h2: 1000, vent_h2s: 160, vent_minerals: 16 } },
  { name: 'Calm, still water', set: { current_strength: 0, wave_strength: 0 } },
  { name: 'Warm world', set: { temp_deep: 18, temp_shallow: 30, temp_beach: 36 } },
  { name: 'Slow, gentle biology (×0.5)', set: { bio_pace: 0.5 } },
];

export class Settings {
  constructor(el, app) {
    this.el = el;
    this.app = app;
    this.rows = new Map();
    this.pending = new Map();
    this.timer = null;
  }

  build(info) {
    this.desc = info.params.desc;
    this.values = { ...info.params.values };
    const groups = new Map();
    for (const d of this.desc) {
      if (!groups.has(d.group)) groups.set(d.group, []);
      groups.get(d.group).push(d);
    }
    const presetOpts = PRESETS.map((p, i) => `<option value="${i}">${esc(p.name)}</option>`).join('');
    this.el.innerHTML = `
      <div class="settings-top"><input type="search" placeholder="Search ${this.desc.length} settings…" id="param-search"><button class="btn small" id="param-reset-all" title="Restore every setting to its default">Reset all</button></div>
      <div class="preset-row"><select id="preset"><option value="">Apply a preset…</option>${presetOpts}</select></div>
      <div id="param-groups"></div>`;
    const box = this.el.querySelector('#param-groups');
    let first = true;
    for (const [name, list] of groups) {
      const det = document.createElement('details');
      det.className = 'group';
      if (first) det.open = true;
      first = false;
      det.innerHTML = `<summary>${esc(name)}<span class="n">${list.length}</span></summary>`;
      for (const d of list) det.appendChild(this.row(d));
      box.appendChild(det);
    }
    this.el.querySelector('#param-search').addEventListener('input', (e) => this.filter(e.target.value));
    this.el.querySelector('#param-reset-all').addEventListener('click', () => {
      this.app.net.send({ cmd: 'reset_params' });
      toast('All settings restored to defaults');
    });
    this.el.querySelector('#preset').addEventListener('change', (e) => {
      const p = PRESETS[Number(e.target.value)];
      e.target.value = '';
      if (!p) return;
      if (p.reset) this.app.net.send({ cmd: 'reset_params' });
      for (const [k, v] of Object.entries(p.set)) this.app.net.send({ cmd: 'set', key: k, value: v });
      toast(`Preset applied: ${p.name}`);
    });
  }

  scaleOf(d) {
    if (d.min > 0 && d.max / d.min >= 50) return 'log';
    if (d.min === 0 && d.max > 0 && d.default > 0 && d.max / d.default >= 15) return 'sq';
    return 'lin';
  }
  toSlider(d, v) {
    const s = this.scaleOf(d);
    if (s === 'log') return (Math.log(v / d.min) / Math.log(d.max / d.min)) * 1000;
    if (s === 'sq') return Math.sqrt(Math.max(0, v - d.min) / (d.max - d.min)) * 1000;
    return ((v - d.min) / (d.max - d.min)) * 1000;
  }
  fromSlider(d, t) {
    t /= 1000;
    const s = this.scaleOf(d);
    if (s === 'log') return d.min * Math.pow(d.max / d.min, t);
    if (s === 'sq') return d.min + t * t * (d.max - d.min);
    return d.min + t * (d.max - d.min);
  }

  row(d) {
    const el = document.createElement('div');
    el.className = 'param';
    el.dataset.search = `${d.label} ${d.key} ${d.desc} ${d.group}`.toLowerCase();
    const isToggle = d.unit === 'on/off';
    const v = this.values[d.key];
    el.innerHTML = `
      <div class="param-top">
        <span class="param-label">${esc(d.label)}${d.reset ? '<span class="pill" title="Takes effect when you create a new world">new world</span>' : ''}</span>
        ${isToggle ? `<input type="checkbox" class="param-check">` : `<input class="param-num" type="number" step="any">`}
        <span class="param-unit">${esc(d.unit)}</span>
        <button class="param-reset" title="Restore default (${fmtVal(d.default)})">↺</button>
      </div>
      ${isToggle ? '' : `<input type="range" min="0" max="1000" step="1">`}
      <div class="param-desc">${esc(d.desc)}</div>`;
    const num = el.querySelector('.param-num');
    const rng = el.querySelector('input[type=range]');
    const chk = el.querySelector('.param-check');
    const send = (val) => {
      val = Math.min(d.max, Math.max(d.min, val));
      this.values[d.key] = val;
      this.refreshRow(d.key);
      this.queue(d.key, val);
    };
    if (rng) rng.addEventListener('input', () => send(this.fromSlider(d, Number(rng.value))));
    if (num) num.addEventListener('change', () => { const x = parseFloat(num.value); if (isFinite(x)) send(x); });
    if (chk) chk.addEventListener('change', () => send(chk.checked ? 1 : 0));
    el.querySelector('.param-reset').addEventListener('click', () => send(d.default));
    this.rows.set(d.key, { el, d, num, rng, chk });
    this.refreshRow(d.key, v);
    return el;
  }

  refreshRow(key) {
    const r = this.rows.get(key);
    if (!r) return;
    const v = this.values[key];
    if (r.num && document.activeElement !== r.num) r.num.value = fmtVal(v);
    if (r.rng && document.activeElement !== r.rng) r.rng.value = this.toSlider(r.d, v);
    if (r.chk) r.chk.checked = v > 0.5;
    r.el.classList.toggle('changed', Math.abs(v - r.d.default) > Math.abs(r.d.default) * 1e-4 + 1e-9);
  }

  queue(key, val) {
    this.pending.set(key, val);
    if (this.timer) return;
    this.timer = setTimeout(() => {
      for (const [k, v] of this.pending) this.app.net.send({ cmd: 'set', key: k, value: v });
      this.pending.clear();
      this.timer = null;
    }, 60);
  }

  update(values) {
    for (const [k, v] of Object.entries(values)) {
      if (this.pending.has(k)) continue;
      this.values[k] = v;
      this.refreshRow(k);
    }
  }

  filter(q) {
    q = q.trim().toLowerCase();
    for (const det of this.el.querySelectorAll('details.group')) {
      let any = false;
      for (const p of det.querySelectorAll('.param')) {
        const show = !q || p.dataset.search.includes(q);
        p.classList.toggle('hidden', !show);
        any = any || show;
      }
      det.classList.toggle('hidden', !any);
      if (q && any) det.open = true;
    }
  }
}

export class EventsPanel {
  constructor(el, app) {
    this.el = el;
    this.app = app;
    this.events = [];
    this.unseen = 0;
    this.badge = document.getElementById('events-badge');
  }

  setAll(events) {
    this.events = events.slice();
    this.render(new Set());
  }

  add(events) {
    const known = new Set(this.events.map((e) => e.seq));
    const fresh = events.filter((e) => !known.has(e.seq)).sort((a, b) => a.seq - b.seq);
    if (!fresh.length) return [];
    this.events.push(...fresh);
    if (this.events.length > 500) this.events.splice(0, this.events.length - 500);
    if (!this.app.tabVisible('events')) {
      this.unseen += fresh.length;
      this.badge.textContent = this.unseen;
      this.badge.classList.remove('hidden');
    }
    this.render(new Set(fresh.map((e) => e.seq)));
    return fresh;
  }

  seen() { this.unseen = 0; this.badge.classList.add('hidden'); }

  render(fresh) {
    if (!this.events.length) {
      this.el.innerHTML = `<div class="empty-state"><h4>No discoveries yet</h4><div>Milestones, new metabolic abilities and new ways of life will appear here as evolution finds them.</div></div>`;
      return;
    }
    const kindName = { discovery: 'New ability', strategy: 'New way of life', milestone: 'Milestone', extinction: 'Extinction' };
    this.el.innerHTML = this.events.slice().reverse().map((e) => {
      const click = e.cell != null || e.x;
      return `<div class="event ${click ? 'clickable' : ''} ${fresh.has(e.seq) ? 'fresh' : ''}" data-seq="${e.seq}">
        <div class="when">${fmtTime(e.t)}</div>
        <div><div class="kind ${e.kind}">${kindName[e.kind] || e.kind}</div><div class="what">${esc(e.text)}</div></div></div>`;
    }).join('');
    this.el.querySelectorAll('.event.clickable').forEach((row) => {
      row.addEventListener('click', () => {
        const ev = this.events.find((x) => x.seq === Number(row.dataset.seq));
        if (ev) this.app.flyTo(ev.x, ev.y, ev.cell);
      });
    });
  }
}

export class Graphs {
  constructor(el, app) {
    this.el = el;
    this.app = app;
    this.charts = [];
    this.dirty = false;
  }

  setInfo(info) {
    this.el.innerHTML = '';
    this.charts = [];
    const strat = info.strategies.slice(0, 7);
    const pop = new Chart(this.el, { title: 'Population by energy source', sub: 'cells', stacked: true, height: 160 });
    pop.setSeries([...strat.map((s, i) => ({ name: s.name, color: s.color, get: (p) => p.strat[i] })), { name: 'No clear source', color: [110, 125, 145], get: (p) => p.strat[9] }]);
    const rates = new Chart(this.el, { title: 'Births and deaths', sub: 'per simulated minute' });
    rates.setSeries([
      { name: 'Births', color: [94, 230, 208], get: (p) => p.birthRate },
      { name: 'Deaths', color: [255, 107, 125], get: (p) => p.deathRate },
    ]);
    const sp = info.species;
    const chemIdx = [1, 2, 3, 4, 5, 6, 9, 7, 8, 0];
    const chemColors = (i) => sp[i].color;
    const chem = new Chart(this.el, { title: 'Dissolved chemistry', sub: 'world total, log scale', log: true, height: 160 });
    chem.hidden = new Set(['CO₂', 'NH₄⁺', 'PO₄³⁻']);
    chem.setSeries(chemIdx.map((i) => ({ name: sp[i].formula, color: chemColors(i), get: (p) => p.chem[i] })));
    const genes = new Chart(this.el, { title: 'Genome size', sub: 'average genes per cell', zero: false, height: 110 });
    genes.setSeries([{ name: 'Genes', color: [201, 167, 255], get: (p) => p.genes }]);
    const lin = new Chart(this.el, { title: 'Surviving founder lineages', sub: 'of the original LUCA cells', height: 110 });
    lin.setSeries([{ name: 'Lineages', color: [127, 176, 255], get: (p) => p.lineages }]);
    const topt = new Chart(this.el, { title: 'Temperature preference', sub: 'average enzyme optimum, °C', zero: false, height: 110 });
    topt.setSeries([{ name: 'T opt', color: [255, 179, 92], get: (p) => p.t_opt }]);
    const size = new Chart(this.el, { title: 'Cell size', sub: 'average area, µm²', zero: false, height: 110 });
    size.setSeries([{ name: 'Size', color: [94, 230, 208], get: (p) => p.size }]);
    const gen = new Chart(this.el, { title: 'Generations', sub: 'longest line of descent', height: 110 });
    gen.setSeries([{ name: 'Generation', color: [255, 210, 122], get: (p) => p.max_gen }]);
    this.charts = [pop, rates, chem, genes, lin, topt, size, gen];
  }

  setHistory(hist) {
    // Derive per-minute birth/death rates from cumulative counters.
    for (let i = 0; i < hist.length; i++) {
      const p = hist[i];
      if (i === 0) { p.birthRate = 0; p.deathRate = 0; continue; }
      const q = hist[i - 1];
      const dt = Math.max(1e-6, p.t - q.t) / 60;
      p.birthRate = Math.max(0, (p.births - q.births) / dt);
      p.deathRate = Math.max(0, (p.deaths - q.deaths) / dt);
    }
    // Rates per sample are noisy: show a centred moving average.
    const win = 6;
    const br = hist.map((p) => p.birthRate), dr = hist.map((p) => p.deathRate);
    for (let i = 0; i < hist.length; i++) {
      let sb = 0, sd = 0, n = 0;
      for (let j = Math.max(1, i - win); j <= Math.min(hist.length - 1, i + win); j++) { sb += br[j]; sd += dr[j]; n++; }
      hist[i].birthRate = n ? sb / n : 0;
      hist[i].deathRate = n ? sd / n : 0;
    }
    this.hist = hist;
    this.dirty = true;
  }

  draw() {
    if (!this.dirty || !this.hist) return;
    this.dirty = false;
    for (const c of this.charts) c.setData(this.hist);
  }
}

export { fmtNum };
