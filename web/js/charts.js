// Lightweight canvas charts for the Graphs tab.

export function fmtTime(s) {
  s = Math.max(0, Math.floor(s));
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), sec = s % 60;
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${String(sec).padStart(2, '0')}` : `${m}:${String(sec).padStart(2, '0')}`;
}

export function fmtNum(v) {
  if (!isFinite(v)) return '—';
  const a = Math.abs(v);
  if (a >= 1e6) return (v / 1e6).toFixed(2) + 'M';
  if (a >= 1e4) return (v / 1e3).toFixed(1) + 'k';
  if (a >= 100) return v.toFixed(0);
  if (a >= 10) return v.toFixed(1);
  if (a >= 1) return v.toFixed(2);
  if (a === 0) return '0';
  if (a >= 0.01) return v.toFixed(3);
  return v.toExponential(1);
}

const css = (c) => `rgb(${c.map((x) => Math.round(x)).join(',')})`;

export class Chart {
  constructor(parent, opts) {
    this.opts = { height: 150, stacked: false, log: false, ...opts };
    this.el = document.createElement('div');
    this.el.className = 'chart';
    this.el.innerHTML = `<div class="chart-head"><span class="chart-title">${opts.title}</span><span class="chart-sub">${opts.sub || ''}</span></div><canvas></canvas><div class="chart-legend"></div>`;
    parent.appendChild(this.el);
    this.canvas = this.el.querySelector('canvas');
    this.canvas.style.height = this.opts.height + 'px';
    this.legend = this.el.querySelector('.chart-legend');
    this.series = [];
    this.hidden = new Set();
    this.hoverX = null;
    this.canvas.addEventListener('mousemove', (e) => {
      const r = this.canvas.getBoundingClientRect();
      this.hoverX = (e.clientX - r.left) / r.width;
      this.draw();
    });
    this.canvas.addEventListener('mouseleave', () => { this.hoverX = null; this.draw(); });
  }

  /// series: [{name, color:[r,g,b], get:(pt)=>number}]
  setSeries(series) {
    this.series = series;
    this.legend.innerHTML = '';
    if (series.length < 2) return;
    for (const s of series) {
      const sp = document.createElement('span');
      sp.innerHTML = `<i style="background:${css(s.color)}"></i>${s.name}`;
      if (this.hidden.has(s.name)) sp.classList.add('off');
      sp.onclick = () => {
        if (this.hidden.has(s.name)) this.hidden.delete(s.name); else this.hidden.add(s.name);
        sp.classList.toggle('off');
        this.draw();
      };
      this.legend.appendChild(sp);
    }
  }

  setData(points) { this.points = points; this.draw(); }

  draw() {
    const cv = this.canvas;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const W = cv.clientWidth, H = cv.clientHeight;
    if (!W || !H) return;
    if (cv.width !== W * dpr) { cv.width = W * dpr; cv.height = H * dpr; }
    const g = cv.getContext('2d');
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, W, H);
    const pts = this.points || [];
    const series = this.series.filter((s) => !this.hidden.has(s.name));
    const pad = { l: 38, r: 8, t: 8, b: 18 };
    const w = W - pad.l - pad.r, h = H - pad.t - pad.b;
    g.font = '10px ui-monospace, SF Mono, Menlo, monospace';
    if (pts.length < 2 || !series.length) {
      g.fillStyle = 'rgba(140,160,180,0.6)';
      g.fillText('collecting data…', pad.l + 8, pad.t + h / 2);
      return;
    }
    const t0 = pts[0].t, t1 = pts[pts.length - 1].t;
    const X = (t) => pad.l + ((t - t0) / Math.max(1e-6, t1 - t0)) * w;
    // y range
    let ymax = 0, ymin = Infinity;
    const vals = series.map((s) => pts.map((p) => Math.max(0, s.get(p) || 0)));
    if (this.opts.stacked) {
      for (let i = 0; i < pts.length; i++) ymax = Math.max(ymax, vals.reduce((a, v) => a + v[i], 0));
      ymin = 0;
    } else {
      for (const v of vals) for (const x of v) { ymax = Math.max(ymax, x); if (x > 0) ymin = Math.min(ymin, x); }
      if (!this.opts.log) ymin = this.opts.zero === false ? Math.min(...vals.flat()) : 0;
    }
    if (ymax <= 0) ymax = 1;
    const log = this.opts.log;
    if (log) { ymin = Math.max(ymin === Infinity ? ymax / 1e3 : ymin, ymax / 1e5); }
    if (!log && this.opts.zero === false) { const span = ymax - ymin || 1; ymin -= span * 0.1; ymax += span * 0.1; }
    const Y = (v) => {
      if (log) {
        const lv = Math.log10(Math.max(v, ymin)), a = Math.log10(ymin), b = Math.log10(ymax);
        return pad.t + h - ((lv - a) / (b - a || 1)) * h;
      }
      return pad.t + h - ((v - ymin) / (ymax - ymin || 1)) * h;
    };
    // grid
    g.strokeStyle = 'rgba(255,255,255,0.06)';
    g.fillStyle = 'rgba(140,160,180,0.7)';
    g.lineWidth = 1;
    const ticks = log ? [ymin, Math.sqrt(ymin * ymax), ymax] : [ymin, (ymin + ymax) / 2, ymax];
    for (const tv of ticks) {
      const y = Math.round(Y(tv)) + 0.5;
      g.beginPath(); g.moveTo(pad.l, y); g.lineTo(pad.l + w, y); g.stroke();
      g.fillText(fmtNum(tv), 2, y + 3);
    }
    g.fillText(fmtTime(t0), pad.l, H - 4);
    const tl = fmtTime(t1);
    g.fillText(tl, pad.l + w - g.measureText(tl).width, H - 4);

    if (this.opts.stacked) {
      const base = new Float64Array(pts.length);
      series.forEach((s, si) => {
        g.beginPath();
        for (let i = 0; i < pts.length; i++) g.lineTo(X(pts[i].t), Y(base[i] + vals[si][i]));
        for (let i = pts.length - 1; i >= 0; i--) g.lineTo(X(pts[i].t), Y(base[i]));
        g.closePath();
        g.fillStyle = `rgba(${s.color.join(',')},0.75)`;
        g.fill();
        for (let i = 0; i < pts.length; i++) base[i] += vals[si][i];
      });
    } else {
      series.forEach((s, si) => {
        g.beginPath();
        for (let i = 0; i < pts.length; i++) {
          const x = X(pts[i].t), y = Y(vals[si][i]);
          if (i === 0) g.moveTo(x, y); else g.lineTo(x, y);
        }
        g.strokeStyle = css(s.color);
        g.lineWidth = 1.6;
        g.stroke();
      });
    }
    if (this.hoverX != null) {
      const tx = t0 + ((this.hoverX * W - pad.l) / w) * (t1 - t0);
      let bi = 0, bd = Infinity;
      for (let i = 0; i < pts.length; i++) { const d = Math.abs(pts[i].t - tx); if (d < bd) { bd = d; bi = i; } }
      const x = X(pts[bi].t);
      g.strokeStyle = 'rgba(255,255,255,0.35)';
      g.beginPath(); g.moveTo(x + 0.5, pad.t); g.lineTo(x + 0.5, pad.t + h); g.stroke();
      const lines = [fmtTime(pts[bi].t), ...series.map((s, si) => [s, vals[si][bi]]).filter(([, v]) => v > 0 || series.length < 4).slice(0, 7).map(([s, v]) => `${s.name}: ${fmtNum(v)}`)];
      const bw = Math.max(...lines.map((l) => g.measureText(l).width)) + 12;
      const bx = x + bw + 10 > W ? x - bw - 6 : x + 6;
      g.fillStyle = 'rgba(5,12,20,0.88)';
      g.fillRect(bx, pad.t, bw, lines.length * 13 + 6);
      g.fillStyle = '#dbe7f3';
      lines.forEach((l, i) => g.fillText(l, bx + 6, pad.t + 13 + i * 13));
    }
  }
}
