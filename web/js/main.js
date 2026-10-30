// Application wiring: connection, camera, input, panels, render loop.

import { Net } from './net.js';
import { Renderer, rampColor } from './renderer.js';
import { Inspector } from './inspector.js';
import { Settings, EventsPanel, Graphs, toast } from './ui.js';
import { fmtTime, fmtNum } from './charts.js';

const $ = (id) => document.getElementById(id);

class App {
  constructor() {
    this.canvas = $('world');
    try {
      this.r = new Renderer(this.canvas);
    } catch (e) {
      document.body.innerHTML = `<div style="padding:40px;font:15px system-ui;color:#ddd">Primordia needs WebGL2, which this browser doesn't provide.<br><br><code>${e.message}</code></div>`;
      throw e;
    }
    this.info = null;
    this.prev = null;
    this.cur = null;
    this.grid = null;
    this.static = null;
    this.selected = null;
    this.selectedLast = null;
    this.hover = null;
    this.follow = false;
    this.paused = false;
    this.speed = { mode: 'every', n: 1 };
    this.history = [];
    this.histVersion = 0;
    this.colorMode = 'strategy';
    this.overlayKey = 'none';
    this.frameInterval = 1 / 60;
    this.lastStats = null;
    this.firstInfo = true;
    this.lineageCache = new Map();

    this.inspector = new Inspector($('tab-cell'), this);
    this.settings = new Settings($('tab-settings'), this);
    this.events = new EventsPanel($('tab-events'), this);
    this.graphs = new Graphs($('tab-graphs'), this);

    this.net = new Net({
      onOpen: () => $('connection').classList.add('hidden'),
      onClose: () => { $('conn-text').textContent = 'Connection lost. Reconnecting… (is Primordia still running?)'; $('connection').classList.remove('hidden'); },
      onJson: (m) => this.onJson(m),
      onCells: (f) => this.onCells(f),
      onGrid: (g) => this.onGrid(g),
      onStatic: (s) => { this.static = s; this.r.setStatic(s); this.buildEnvTextures(); this.drawMinimapBase(); },
    });
    $('connection').classList.remove('hidden');
    this.net.connect();
    this.bindUI();
    this.bindInput();
    this.last = performance.now();
    requestAnimationFrame((t) => this.loop(t));
    setInterval(() => this.pollInspector(), 450);
    setInterval(() => this.drawMinimap(), 250);
  }

  // ------------------------------------------------------------------ data

  onJson(m) {
    switch (m.type) {
      case 'info': return this.onInfo(m);
      case 'stats': return this.onStats(m);
      case 'params': return this.onParams(m.values);
      case 'inspect':
        if (m.id !== this.selected) return;
        if (m.found) { this.inspector.render(m); this.selectedLast = m; }
        else this.onSelectedGone();
        return;
      case 'history':
        this.history = m.history; this.histVersion = m.hist_version;
        this.graphs.setHistory(this.history);
        return;
      case 'saves': return this.showSaves(m.files);
      case 'toast': return toast(m.text, m.ok ? '' : 'error');
    }
  }

  onInfo(info) {
    const reset = !this.info || this.info.seed !== info.seed || info.time < (this.info.time || 0);
    this.info = info;
    this.r.setWorld(info);
    this.paused = info.paused;
    this.speed = info.speed;
    this.history = info.history || [];
    this.histVersion = info.hist_version;
    this.params = { ...info.params.values };
    this.applyParamsToRenderer();
    this.buildOverlayOptions();
    this.settings.build(info);
    this.events.setAll(info.events || []);
    this.graphs.setInfo(info);
    this.graphs.setHistory(this.history);
    this.updateColorLegend();
    this.syncTransport();
    this.proj = null;
    if (reset) {
      this.prev = this.cur = null;
      this.select(null);
      this.r.bursts = [];
    }
    if (this.firstInfo || reset) {
      this.r.fitWorld();
      this.r.cam = { ...this.r.camTarget };
      this.firstInfo = false;
    }
    document.title = `Primordia · seed ${info.seed}`;
  }

  onParams(values) {
    this.params = { ...this.params, ...values };
    this.settings.update(values);
    this.applyParamsToRenderer();
    this.buildEnvTextures();
    this.refreshOverlay();
  }

  applyParamsToRenderer() {
    const p = this.params;
    this.r.light = [p.light_deep, p.light_shallow, p.light_beach];
    this.r.wavePeriod = p.wave_period;
    this.r.ventRadius = p.vent_radius;
  }

  onStats(s) {
    this.lastStats = s;
    this.paused = s.paused;
    this.speed = s.speed;
    this.syncTransport();
    const l = s.latest;
    if (l) {
      $('ro-pop').textContent = l.pop.toLocaleString();
      $('ro-gen').textContent = l.max_gen.toLocaleString();
      $('ro-lin').textContent = l.lineages;
    }
    $('ro-time').textContent = fmtTime(s.time);
    const rt = s.sps / 60;
    $('ro-speed').textContent = this.paused ? 'paused' : `${rt >= 10 ? rt.toFixed(0) : rt.toFixed(1)}× real time`;
    $('ro-speed').title = `${Math.round(s.sps)} simulation steps per second`;
    this.r.daylight = s.daylight;
    // History: append the newest point, or resync if we fell out of step.
    if (s.hist_version !== this.histVersion) {
      if (s.hist_version === this.histVersion + 1 && s.last_point) {
        this.history.push(s.last_point);
        this.histVersion = s.hist_version;
        this.graphs.setHistory(this.history);
      } else {
        this.histVersion = s.hist_version;
        this.net.send({ cmd: 'history' });
      }
    }
    const fresh = this.events.add(s.events || []);
    for (const e of fresh) {
      if (e.kind === 'discovery' || e.kind === 'strategy' || e.kind === 'extinction') toast(e.text, 'discovery');
    }
    this.updateColorLegend();
  }

  onCells(f) {
    const now = performance.now();
    if (this.cur) {
      const dt = (now - this.cur.arrived) / 1000;
      if (dt > 0 && dt < 0.5) this.frameInterval += (dt - this.frameInterval) * 0.2;
    }
    // While paused, let cosmetic animation follow single steps.
    if (this.paused && this.cur && f.time > this.cur.time) this.r.time += Math.min(0.5, f.time - this.cur.time);
    this.prev = this.cur;
    this.cur = f;
    if (f.deaths.length) this.r.addBursts(f.deaths);
  }

  onGrid(g) {
    this.grid = g;
    this.r.daylight = g.daylight;
    this.refreshOverlay();
  }

  // ---------------------------------------------------------------- overlays

  buildOverlayOptions() {
    const sel = $('overlay');
    const sp = this.info.species;
    const opts = [['none', 'None']];
    [3, 1, 2, 4, 5, 6, 7, 8, 9, 0].forEach((i) => opts.push([`s${i}`, `${sp[i].formula} · ${sp[i].name}`]));
    opts.push(['temp', 'Temperature'], ['light', 'Sunlight'], ['flow', 'Water currents']);
    sel.innerHTML = opts.map(([v, l]) => `<option value="${v}">${l}</option>`).join('');
    sel.value = this.overlayKey;
  }

  buildEnvTextures() {
    if (!this.static || !this.params) return;
    const p = this.params, st = this.static, n = st.gw * st.gh;
    this.tempTex = new Uint8Array(n);
    this.lightTex = new Uint8Array(n);
    this.tempField = new Float32Array(n);
    for (let i = 0; i < n; i++) {
      const z0 = st.rgba[i * 4] / 255, z1 = st.rgba[i * 4 + 1] / 255, z2 = st.rgba[i * 4 + 2] / 255, h = st.rgba[i * 4 + 3] / 255;
      const tot = z0 + z1 + z2 || 1;
      const base = (z0 * p.temp_deep + z1 * p.temp_shallow + z2 * p.temp_beach) / tot;
      const t = base + h * (p.vent_temp - base);
      this.tempField[i] = t;
      this.tempTex[i] = Math.max(0, Math.min(255, ((t + 2) / 92) * 255));
      this.lightTex[i] = Math.max(0, Math.min(255, ((z0 * p.light_deep + z1 * p.light_shallow + z2 * p.light_beach) / tot) * 255));
    }
  }

  refreshOverlay() {
    const k = this.overlayKey;
    const leg = $('overlay-legend');
    if (k === 'none') { this.r.setOverlay(null); leg.classList.add('hidden'); return; }
    leg.classList.remove('hidden');
    const ramp = $('overlay-ramp');
    if (k.startsWith('s')) {
      if (!this.grid) return;
      const s = Number(k.slice(1));
      const col = this.info.species[s].color.map((x) => x / 255);
      this.r.setOverlay(this.grid.planes[s], this.grid.gw, this.grid.gh, 0, col);
      ramp.style.background = `linear-gradient(90deg, #05050d, rgb(${this.info.species[s].color.join(',')}), #fff)`;
      $('ov-min').textContent = '0';
      $('ov-max').textContent = `${fmtNum(this.grid.scales[s])} units/µm²`;
    } else if (k === 'temp') {
      if (!this.tempTex) return;
      this.r.setOverlay(this.tempTex, this.static.gw, this.static.gh, 1, [1, 1, 1]);
      ramp.style.background = 'linear-gradient(90deg, rgb(26,64,217), rgb(51,204,179), rgb(255,191,51), rgb(255,51,26))';
      $('ov-min').textContent = '−2 °C'; $('ov-max').textContent = '90 °C';
    } else if (k === 'light') {
      if (!this.lightTex) return;
      const day = this.r.daylight ?? 1;
      const data = day >= 0.999 ? this.lightTex : this.lightTex.map((v) => v * day);
      this.r.setOverlay(data, this.static.gw, this.static.gh, 2, [1, 1, 1]);
      ramp.style.background = 'linear-gradient(90deg, #050508, rgb(255,237,153))';
      $('ov-min').textContent = 'dark'; $('ov-max').textContent = 'full sun';
    } else if (k === 'flow') {
      if (!this.grid) return;
      const f = this.grid.flow, n = f.u.length, d = new Uint8Array(n);
      for (let i = 0; i < n; i++) d[i] = Math.min(255, Math.hypot(f.u[i], f.v[i]) * 2);
      this.r.setOverlay(d, this.grid.gw, this.grid.gh, 3, [1, 1, 1]);
      ramp.style.background = 'linear-gradient(90deg, rgb(13,26,51), rgb(153,230,255))';
      $('ov-min').textContent = 'still'; $('ov-max').textContent = `${fmtNum(f.vmax)} µm/s`;
    }
  }

  // ---------------------------------------------------------------- colours

  colorFn() {
    const mode = this.colorMode;
    if (mode === 'strategy') return (u8, o) => [u8[o + 44] / 255, u8[o + 45] / 255, u8[o + 46] / 255];
    if (mode === 'lineage') return (u8, o) => {
      const h = u8[o + 47];
      let c = this.lineageCache.get(h);
      if (!c) { c = Renderer.lineageColor(h / 255); this.lineageCache.set(h, c); }
      return c;
    };
    if (mode === 'energy') return (u8, o) => rampColor(u8[o + 38] / 255);
    if (mode === 'damage') return (u8, o) => rampColor(1 - u8[o + 39] / 255, [[0.95, 0.25, 0.3], [0.95, 0.7, 0.25], [0.35, 0.9, 0.6]]);
    if (mode === 'genes') return (u8, o) => rampColor(Math.min(1, (u8[o + 42] - 2) / 16));
    return () => [1, 1, 1];
  }

  updateColorLegend() {
    const el = $('color-legend');
    if (!this.info) return;
    const mode = this.colorMode;
    if (mode === 'strategy') {
      const l = this.lastStats?.latest;
      const rows = this.info.strategies.slice(0, 7).map((s, i) => [s, l ? l.strat[i] : 0]);
      rows.push([{ name: 'No clear source', color: [120, 135, 160] }, l ? l.strat[9] : 0]);
      el.innerHTML = rows.filter(([, n], i) => n > 0 || i < 1).map(([s, n]) => `<div class="legend-item"><span class="sw" style="background:rgb(${s.color.join(',')});color:rgb(${s.color.join(',')})"></span>${s.name}<span class="count">${n ? n.toLocaleString() : ''}</span></div>`).join('') +
        `<div class="legend-item" style="color:var(--faint);font-size:11px;margin-top:3px">Only strategies in use are listed</div>`;
    } else {
      const labels = { lineage: ['Similar colours are close relatives', null], energy: ['Starving', 'Full of ATP'], damage: ['Healthy', 'Badly damaged'], genes: ['2 genes', '18+ genes'] }[mode];
      if (mode === 'lineage') { el.innerHTML = `<div class="legend-item">${labels[0]}. Colours drift a little with every mutation.</div>`; return; }
      const stops = mode === 'damage' ? 'linear-gradient(90deg, rgb(89,230,153), rgb(242,179,64), rgb(242,64,77))' : 'linear-gradient(90deg, rgb(69,0,84), rgb(59,82,140), rgb(33,145,140), rgb(94,201,97), rgb(252,232,36))';
      el.innerHTML = `<div class="ramp" style="background:${stops}"></div><div class="legend-labels"><span>${labels[0]}</span><span>${labels[1]}</span></div>`;
    }
  }

  // ---------------------------------------------------------------- selection

  select(id) {
    this.selected = id;
    this.selectedLast = null;
    if (id == null) { this.follow = false; this.inspector.clear(); return; }
    this.showTab('cell');
    this.net.send({ cmd: 'inspect', id });
  }

  toggleFollow() {
    this.follow = !this.follow;
    if (this.inspector.data) this.inspector.render(this.inspector.data);
    toast(this.follow ? 'Following this cell (and its descendants)' : 'Stopped following');
  }

  pollInspector() {
    if (this.selected != null && this.tabVisible('cell')) this.net.send({ cmd: 'inspect', id: this.selected });
  }

  onSelectedGone() {
    const last = this.selectedLast;
    if (this.follow && last && this.cur) {
      // Jump to the nearest daughter (one generation on) if it divided.
      const lc = last.cell;
      let best = null, bd = Infinity;
      for (let i = 0; i < this.cur.n; i++) {
        const o = 32 + i * 52;
        const gen = this.cur.u8[o + 51];
        if (gen !== ((lc.generation + 1) & 255)) continue;
        const dx = this.cur.pos[i * 4] - lc.x, dy = this.cur.pos[i * 4 + 1] - lc.y;
        const d = Math.hypot(dx, dy);
        if (d < bd && d < lc.radius * 4 + 15) { bd = d; best = this.cur.ids[i]; }
      }
      if (best != null) {
        this.selected = best;
        this.net.send({ cmd: 'inspect', id: best });
        return;
      }
    }
    this.inspector.gone(last);
  }

  flyTo(x, y, id) {
    this.r.camTarget = { x, y, z: Math.max(this.r.camTarget.z, 9) };
    if (id != null && this.cur && this.cur.index.has(id)) this.select(id);
  }

  projection() {
    if (this.proj) return this.proj;
    // Principal components of the function sites, for the shape-space plot.
    const S = this.info.sites, n = S.length, D = S[0].length;
    const mean = Array(D).fill(0);
    S.forEach((s) => s.forEach((v, k) => (mean[k] += v / n)));
    const C = Array.from({ length: D }, () => Array(D).fill(0));
    S.forEach((s) => { for (let a = 0; a < D; a++) for (let b = 0; b < D; b++) C[a][b] += (s[a] - mean[a]) * (s[b] - mean[b]); });
    const power = (M, deflate) => {
      let v = Array.from({ length: D }, (_, i) => Math.sin(i * 2.3 + 1));
      for (let it = 0; it < 100; it++) {
        let w = M.map((row) => row.reduce((acc, x, k) => acc + x * v[k], 0));
        if (deflate) { const d = w.reduce((a, x, k) => a + x * deflate[k], 0); w = w.map((x, k) => x - d * deflate[k]); }
        const len = Math.hypot(...w) || 1;
        v = w.map((x) => x / len);
      }
      return v;
    };
    const p1 = power(C), p2 = power(C, p1);
    this.proj = [p1, p2];
    return this.proj;
  }

  // ---------------------------------------------------------------- UI

  tabVisible(name) { return !$(`tab-${name}`).classList.contains('hidden'); }

  showTab(name) {
    document.querySelectorAll('.tab').forEach((t) => t.classList.toggle('active', t.dataset.tab === name));
    document.querySelectorAll('.tab-body').forEach((b) => b.classList.toggle('hidden', b.id !== `tab-${name}`));
    if (name === 'events') this.events.seen();
    if (name === 'graphs') { this.graphs.dirty = true; }
  }

  syncTransport() {
    $('btn-play').classList.toggle('paused', !!this.paused);
    const seg = $('speed-seg');
    seg.querySelectorAll('button').forEach((b) => {
      const v = b.dataset.speed;
      let on = false;
      if (this.speed.mode === 'max') on = v === 'max';
      else if (this.speed.n === 1) on = v === '1';
      else if (this.speed.n === 5) on = v === '5';
      else on = v === 'custom';
      b.classList.toggle('active', on);
    });
    if (this.speed.mode === 'every' && this.speed.n !== 1 && this.speed.n !== 5 && document.activeElement !== $('speed-n')) $('speed-n').value = this.speed.n;
  }

  setSpeed(v) {
    if (v === 'max') this.net.send({ cmd: 'speed', mode: 'max' });
    else if (v === 'custom') this.net.send({ cmd: 'speed', mode: 'every', n: Math.max(2, Math.min(1000, Number($('speed-n').value) || 20)) });
    else this.net.send({ cmd: 'speed', mode: 'every', n: Number(v) });
  }

  togglePanel() {
    const collapsed = document.body.classList.toggle('panel-collapsed');
    $('panel-toggle').textContent = collapsed ? '‹' : '›';
    this.r.fitMarginRight = collapsed ? 20 : 410;
  }

  togglePause() {
    this.paused = !this.paused;
    this.net.send({ cmd: this.paused ? 'pause' : 'resume' });
    this.syncTransport();
  }

  bindUI() {
    $('btn-play').onclick = () => this.togglePause();
    $('btn-step').onclick = () => {
      if (!this.paused) this.togglePause();
      this.net.send({ cmd: 'step', n: this.speed.mode === 'every' ? this.speed.n : 30 });
    };
    $('speed-seg').addEventListener('click', (e) => {
      const b = e.target.closest('button');
      if (!b || e.target.tagName === 'INPUT') { if (e.target.tagName === 'INPUT') this.setSpeed('custom'); return; }
      this.setSpeed(b.dataset.speed);
    });
    $('speed-n').addEventListener('change', () => this.setSpeed('custom'));
    document.querySelectorAll('.tab').forEach((t) => (t.onclick = () => this.showTab(t.dataset.tab)));
    $('overlay').onchange = (e) => { this.overlayKey = e.target.value; this.refreshOverlay(); };
    $('colormode').onchange = (e) => { this.colorMode = e.target.value; this.updateColorLegend(); };
    $('tg-particles').onchange = (e) => (this.r.particlesOn = e.target.checked);
    $('tg-flagella').onchange = (e) => (this.r.detail = e.target.checked);
    $('tg-deaths').onchange = (e) => { this.r.burstsOn = e.target.checked; if (!e.target.checked) this.r.bursts = []; };
    $('tg-zones').onchange = (e) => $('zone-labels').classList.toggle('hidden', !e.target.checked);
    $('panel-toggle').onclick = () => this.togglePanel();

    const dlgNew = $('dlg-new');
    $('btn-new').onclick = () => { $('new-seed').value = ''; dlgNew.showModal(); };
    dlgNew.addEventListener('close', () => {
      if (dlgNew.returnValue !== 'ok') return;
      const s = $('new-seed').value.trim();
      this.net.send({ cmd: 'new_world', ...(s ? { seed: Number(s) } : {}) });
      toast('Creating a new world…');
    });
    const dlgSave = $('dlg-save');
    $('btn-save').onclick = () => { $('save-name').value = this.info ? `seed${this.info.seed}-${fmtTime(this.lastStats?.time || 0).replace(/:/g, 'm')}` : ''; dlgSave.showModal(); };
    dlgSave.addEventListener('close', () => {
      if (dlgSave.returnValue === 'ok') this.net.send({ cmd: 'save', name: $('save-name').value.trim() || undefined });
    });
    $('btn-load').onclick = () => { $('save-list').innerHTML = '<div class="muted">Loading…</div>'; $('dlg-load').showModal(); this.net.send({ cmd: 'list_saves' }); };
  }

  showSaves(files) {
    const list = $('save-list');
    if (!files.length) { list.innerHTML = '<div class="muted">No saved worlds yet. Use Save to keep a world you like.</div>'; return; }
    list.innerHTML = files.map((f) => `<button type="button" class="save-item" data-name="${f.name}"><span class="nm">${f.name}</span><span class="meta">${(f.size / 1e6).toFixed(1)} MB · ${new Date(f.modified * 1000).toLocaleString()}</span></button>`).join('');
    list.querySelectorAll('.save-item').forEach((b) => (b.onclick = () => { this.net.send({ cmd: 'load', name: b.dataset.name }); $('dlg-load').close(); }));
  }

  bindInput() {
    const cv = this.canvas;
    let drag = null;
    cv.addEventListener('pointerdown', (e) => {
      cv.setPointerCapture(e.pointerId);
      drag = { x: e.clientX, y: e.clientY, cx: this.r.camTarget.x, cy: this.r.camTarget.y, moved: false };
    });
    cv.addEventListener('pointermove', (e) => {
      this.mouse = [e.clientX, e.clientY];
      if (drag) {
        const dx = e.clientX - drag.x, dy = e.clientY - drag.y;
        if (Math.abs(dx) + Math.abs(dy) > 3) { drag.moved = true; cv.classList.add('dragging'); if (this.follow) this.follow = false; }
        if (drag.moved) {
          const z = this.r.cam.z;
          this.r.camTarget.x = drag.cx - dx / z;
          this.r.camTarget.y = drag.cy - dy / z;
          this.r.cam.x = this.r.camTarget.x; this.r.cam.y = this.r.camTarget.y;
        }
      }
    });
    cv.addEventListener('pointerup', (e) => {
      cv.classList.remove('dragging');
      if (drag && !drag.moved) {
        const i = this.r.pick(e.clientX, e.clientY);
        this.select(i >= 0 ? this.cur.ids[i] : null);
      }
      drag = null;
    });
    cv.addEventListener('pointerleave', () => { this.mouse = null; });
    cv.addEventListener('dblclick', (e) => {
      const [wx, wy] = this.r.screenToWorld(e.clientX, e.clientY);
      this.r.camTarget = { x: wx, y: wy, z: Math.min(80, this.r.camTarget.z * 2.5) };
    });
    cv.addEventListener('wheel', (e) => {
      e.preventDefault();
      const scale = e.ctrlKey ? 0.012 : 0.0018;
      const f = Math.exp(-e.deltaY * scale * (e.deltaMode === 1 ? 16 : 1));
      const t = this.r.camTarget;
      const nz = Math.max(0.25, Math.min(120, t.z * f));
      // Keep the point under the cursor fixed.
      const sx = e.clientX - cv.clientWidth / 2, sy = e.clientY - cv.clientHeight / 2;
      const wx = t.x + sx / t.z, wy = t.y + sy / t.z;
      t.x = wx - sx / nz; t.y = wy - sy / nz; t.z = nz;
    }, { passive: false });
    window.addEventListener('keydown', (e) => {
      if (e.target.tagName === 'INPUT' || e.target.tagName === 'SELECT' || e.target.tagName === 'TEXTAREA') return;
      const t = this.r.camTarget;
      switch (e.key) {
        case ' ': e.preventDefault(); this.togglePause(); break;
        case '.': $('btn-step').click(); break;
        case '1': this.setSpeed('1'); break;
        case '2': this.setSpeed('5'); break;
        case '3': this.setSpeed('custom'); break;
        case '4': this.setSpeed('max'); break;
        case 'f': case 'F': if (this.selected != null) this.toggleFollow(); break;
        case 'Escape': this.select(null); break;
        case 'h': case 'H': document.body.classList.toggle('ui-hidden'); break;
        case 'p': case 'P': this.togglePanel(); break;
        case '0': this.r.fitWorld(); break;
        case '+': case '=': t.z = Math.min(120, t.z * 1.3); break;
        case '-': t.z = Math.max(0.25, t.z / 1.3); break;
        case 'ArrowLeft': t.x -= 80 / t.z; break;
        case 'ArrowRight': t.x += 80 / t.z; break;
        case 'ArrowUp': t.y -= 80 / t.z; break;
        case 'ArrowDown': t.y += 80 / t.z; break;
      }
    });
    window.addEventListener('resize', () => this.r.resize());
    $('minimap').addEventListener('click', (e) => {
      if (!this.info) return;
      const rc = e.target.getBoundingClientRect();
      const fx = (e.clientX - rc.left) / rc.width, fy = (e.clientY - rc.top) / rc.height;
      this.r.camTarget.x = fx * this.info.world.w;
      this.r.camTarget.y = fy * this.info.world.h;
      this.follow = false;
    });
  }

  // ---------------------------------------------------------------- HUD

  drawMinimapBase() {
    const st = this.static;
    if (!st) return;
    const c = document.createElement('canvas');
    c.width = st.gw; c.height = st.gh;
    const g = c.getContext('2d');
    const img = g.createImageData(st.gw, st.gh);
    for (let i = 0; i < st.gw * st.gh; i++) {
      const z0 = st.rgba[i * 4] / 255, z1 = st.rgba[i * 4 + 1] / 255, z2 = st.rgba[i * 4 + 2] / 255, h = st.rgba[i * 4 + 3] / 255;
      img.data[i * 4] = 8 * z0 + 12 * z1 + 70 * z2 + 180 * h * h;
      img.data[i * 4 + 1] = 20 * z0 + 60 * z1 + 62 * z2 + 70 * h * h;
      img.data[i * 4 + 2] = 45 * z0 + 72 * z1 + 48 * z2 + 20 * h * h;
      img.data[i * 4 + 3] = 255;
    }
    g.putImageData(img, 0, 0);
    this.miniBase = c;
  }

  drawMinimap() {
    const cv = $('minimap');
    if (!this.miniBase || !this.info || document.body.classList.contains('ui-hidden')) return;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const W = cv.clientWidth, H = cv.clientHeight;
    if (cv.width !== W * dpr) { cv.width = W * dpr; cv.height = H * dpr; }
    const g = cv.getContext('2d');
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.imageSmoothingEnabled = true;
    g.drawImage(this.miniBase, 0, 0, W, H);
    const { w, h } = this.info.world;
    if (this.cur) {
      const f = this.cur, cf = this.colorFn();
      for (let i = 0; i < f.n; i++) {
        const c = cf(f.u8, 32 + i * 52);
        g.fillStyle = `rgb(${c[0] * 255 | 0},${c[1] * 255 | 0},${c[2] * 255 | 0})`;
        g.fillRect((f.pos[i * 4] / w) * W - 0.5, (f.pos[i * 4 + 1] / h) * H - 0.5, 1.4, 1.4);
      }
    }
    const [x0, y0] = this.r.screenToWorld(0, 0);
    const [x1, y1] = this.r.screenToWorld(this.canvas.clientWidth, this.canvas.clientHeight);
    g.strokeStyle = 'rgba(94,230,208,0.9)';
    g.lineWidth = 1.2;
    g.strokeRect((x0 / w) * W, (y0 / h) * H, ((x1 - x0) / w) * W, ((y1 - y0) / h) * H);
  }

  updateHud() {
    const z = this.r.cam.z;
    // Scale bar with a "nice" length.
    const target = 110 / z;
    const pow = Math.pow(10, Math.floor(Math.log10(target)));
    const nice = [1, 2, 5, 10].map((m) => m * pow).reduce((a, b) => (Math.abs(b - target) < Math.abs(a - target) ? b : a));
    document.querySelector('#scalebar .bar').style.width = `${nice * z}px`;
    $('scalebar-label').textContent = nice >= 1000 ? `${nice / 1000} mm` : `${nice} µm`;

    // Zone labels along the top of the world.
    if (this.info && !$('zone-labels').classList.contains('hidden')) {
      const W = this.info.world;
      const zones = [['Deep ocean', W.deep_end / 2], ['Shallow ocean', (W.deep_end + W.shallow_end) / 2], ['Beach', (W.shallow_end + 1) / 2]];
      const top = Math.max(78, this.r.worldToScreen(0, 0)[1] + 10);
      const box = $('zone-labels');
      if (box.children.length !== 3) box.innerHTML = zones.map(() => '<div class="zone-label"></div>').join('');
      zones.forEach(([name, fx], i) => {
        const [sx] = this.r.worldToScreen(fx * W.w, 0);
        const el = box.children[i];
        el.textContent = name;
        el.style.left = `${sx}px`;
        el.style.top = `${top}px`;
        el.style.opacity = z > 6 ? 0 : 1;
      });
    }

    // What's under the cursor.
    const ro = $('cursor-readout');
    if (this.mouse && this.static && this.info) {
      const [wx, wy] = this.r.screenToWorld(...this.mouse);
      const W = this.info.world;
      if (wx >= 0 && wy >= 0 && wx <= W.w && wy <= W.h) {
        const gx = Math.min(this.static.gw - 1, (wx / W.dx) | 0), gy = Math.min(this.static.gh - 1, (wy / W.dx) | 0);
        const gi = gy * this.static.gw + gx;
        const st = this.static.rgba;
        const zi = [st[gi * 4], st[gi * 4 + 1], st[gi * 4 + 2]];
        const zone = ['deep ocean', 'shallow ocean', 'beach'][zi.indexOf(Math.max(...zi))];
        let txt = `${wx.toFixed(0)}, ${wy.toFixed(0)} µm · ${zone} · ${this.tempField ? this.tempField[gi].toFixed(1) : '—'}°C`;
        if (this.overlayKey.startsWith('s') && this.grid) {
          const s = Number(this.overlayKey.slice(1));
          const v = Math.pow(this.grid.planes[s][gi] / 255, 2) * this.grid.scales[s];
          txt += ` · ${this.info.species[s].formula} ${fmtNum(v)}`;
        }
        ro.textContent = txt;
      } else ro.textContent = '';
    } else ro.textContent = '';
  }

  updateTooltip() {
    const tt = $('tooltip');
    if (!this.mouse || !this.cur || !this.info) { tt.classList.add('hidden'); this.hover = null; return; }
    const i = this.r.pick(...this.mouse);
    if (i < 0) { tt.classList.add('hidden'); this.hover = null; return; }
    const id = this.cur.ids[i];
    this.hover = id;
    const o = 32 + i * 52, u8 = this.cur.u8;
    const c = [u8[o + 44], u8[o + 45], u8[o + 46]];
    let best = 'idle', bd = 1e9;
    this.info.strategies.forEach((s) => {
      const d = (s.color[0] - c[0]) ** 2 + (s.color[1] - c[1]) ** 2 + (s.color[2] - c[2]) ** 2;
      if (d < bd) { bd = d; best = s.name; }
    });
    if (bd > 2500) best = 'mixed metabolism';
    const area = Math.PI * this.cur.pos[i * 4 + 2] ** 2;
    tt.innerHTML = `<div class="tt-title">Cell #${id}</div><div class="tt-row">${best}</div><div class="tt-row">${area.toFixed(1)} µm² · gen ${u8[o + 51]}${u8[o + 51] === 255 ? '+' : ''} · ${u8[o + 42]} genes</div><div class="tt-row">ATP ${(u8[o + 38] / 2.55).toFixed(0)}% · damage ${(u8[o + 39] / 2.55).toFixed(0)}%</div>`;
    tt.style.left = `${this.mouse[0] + 14}px`;
    tt.style.top = `${this.mouse[1] + 14}px`;
    tt.classList.remove('hidden');
  }

  // ---------------------------------------------------------------- loop

  loop(t) {
    const dt = Math.min(0.1, (t - this.last) / 1000);
    this.last = t;
    let alpha = 1;
    if (this.cur && this.prev) {
      alpha = Math.min(1, Math.max(0, (performance.now() - this.cur.arrived) / 1000 / Math.max(0.005, this.frameInterval)));
    }
    // Keep the camera on the followed cell.
    if (this.follow && this.selected != null && this.cur) {
      const i = this.cur.index.get(this.selected);
      if (i !== undefined) {
        let x = this.cur.pos[i * 4], y = this.cur.pos[i * 4 + 1];
        const j = this.prev?.index.get(this.selected);
        if (j !== undefined) { x = this.prev.pos[j * 4] + (x - this.prev.pos[j * 4]) * alpha; y = this.prev.pos[j * 4 + 1] + (y - this.prev.pos[j * 4 + 1]) * alpha; }
        this.r.camTarget.x = x; this.r.camTarget.y = y;
      }
    }
    this.r.buildInstances(this.prev, this.cur, alpha, this.colorFn(), { selected: this.selected, hover: this.hover });
    this.r.draw(dt, !this.paused, this.grid ? { ...this.grid.flow, gw: this.grid.gw, gh: this.grid.gh } : null);
    this.updateHud();
    this.updateTooltip();
    if (this.tabVisible('graphs')) this.graphs.draw();
    requestAnimationFrame((tt) => this.loop(tt));
  }
}

window.app = new App();
