// The Cell tab: everything about one selected cell, down to its codons.

import { fmtNum, fmtTime } from './charts.js';

const css = (c) => `rgb(${c.map((x) => Math.round(x)).join(',')})`;
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));

function meter(name, frac, val, color = 'var(--accent)') {
  const w = Math.max(0, Math.min(1, frac)) * 100;
  return `<div class="meter"><span class="name">${name}</span><div class="track"><div class="fill" style="width:${w}%;background:${color}"></div></div><span class="val">${val}</span></div>`;
}

const EMPTY = `<div class="empty-state">
  <svg viewBox="0 0 64 64" fill="none" stroke="#5ee6d0" stroke-width="1.5"><circle cx="32" cy="32" r="22" opacity=".8"/><circle cx="27" cy="29" r="8" fill="#5ee6d0" fill-opacity=".15"/><circle cx="40" cy="38" r="2.5"/><path d="M54 32c4 2 6 6 7 10" opacity=".5"/></svg>
  <h4>Click any cell</h4>
  <div>See its energy, metabolism, proteins and genome, codon by codon.<br><br>Scroll to zoom in, drag to move around.</div>
</div>`;

export class Inspector {
  constructor(el, app) {
    this.el = el;
    this.app = app;
    this.data = null;
    this.open = new Set();
    this.el.innerHTML = EMPTY;
    this.el.addEventListener('click', (e) => {
      const t = e.target.closest('[data-act]');
      if (!t) return;
      const act = t.dataset.act;
      if (act === 'follow') this.app.toggleFollow();
      else if (act === 'close') this.app.select(null);
      else if (act === 'seq') {
        const g = Number(t.dataset.gene);
        if (this.open.has(g)) this.open.delete(g); else this.open.add(g);
        t.closest('.gene').classList.toggle('open');
        t.textContent = this.open.has(g) ? 'hide sequence' : 'show sequence';
      }
    });
  }

  clear() { this.data = null; this.el.innerHTML = EMPTY; }

  gone(lastData) {
    const msg = lastData ? `<div class="card"><h4>Cell #${lastData.cell.id}</h4><div class="muted">This cell is gone: it divided or died. Click another cell, or turn on <b>Follow</b> to keep tracking a lineage automatically.</div></div>` : EMPTY;
    this.el.innerHTML = msg;
  }

  render(d) {
    const info = this.app.info;
    if (!info) return;
    this.data = d;
    const c = d.cell, g = d.genome;
    const strat = info.strategies;
    const species = info.species;
    const reactions = info.reactions;
    const scroll = this.el.scrollTop;

    const mix = c.strategy_mix.map((v, i) => [v, i]).filter(([v]) => v > 0.005).sort((a, b) => b[0] - a[0]);
    const stack = mix.map(([v, i]) => `<div style="width:${v * 100}%;background:${css(strat[i].color)}" title="${strat[i].name}"></div>`).join('');
    const mixList = mix.map(([v, i]) => `<div class="legend-item"><span class="sw" style="background:${css(strat[i].color)};color:${css(strat[i].color)}"></span>${strat[i].name}<span class="count">${(v * 100).toFixed(0)}%</span></div>`).join('');

    const maxFlux = Math.max(1e-9, ...c.flux);
    const rx = c.flux.map((f, i) => [f, i]).filter(([f]) => f > 1e-4).sort((a, b) => b[0] - a[0]).map(([f, i]) => {
      const r = reactions[i];
      const col = strat[r.strategy] ? css(strat[r.strategy].color) : 'var(--accent2)';
      const atp = r.atp > 0 ? `+${r.atp.toFixed(1)} ATP` : r.atp < 0 ? `${r.atp.toFixed(1)} ATP` : '';
      return `<div class="rxn"><span class="nm">${r.name}</span><div class="track"><div class="fill" style="width:${(f / maxFlux) * 100}%;background:${col}"></div></div><span class="eq">${esc(r.equation)} ${atp ? '· ' + atp : ''} · ${fmtNum(f)}/µm²/s</span></div>`;
    }).join('') || '<div class="muted">No active reactions.</div>';

    const fnName = (f) => info.functions[f];
    const genes = g.genes.map((ge) => {
      const best = ge.acts[0];
      const junk = !best || best.a < 0.1;
      const fn = junk ? 'No clear function (junk / drifting)' : fnName(best.f);
      const side = ge.acts.slice(junk ? 0 : 1, 5).filter((a) => a.a > 0.02).map((a) => `<span class="chip">${esc(fnName(a.f))} <b>${(a.a * 100).toFixed(0)}%</b></span>`).join('');
      const mem = ge.membrane > 0.5;
      const reg = ge.reg ? `<div class="reg-line">Regulator: senses ${esc(ge.reg.ligand)} · ${ge.reg.activator ? 'activates' : 'represses'} ${ge.reg.targets.length ? 'genes ' + ge.reg.targets.map((t) => '#' + t.gene).join(', ') : 'no genes here'} ${ge.reg.ligand_activates ? 'when present' : 'when absent'}</div>` : '';
      const regf = Math.abs(ge.reg_factor - 1) > 0.02 ? ` · regulated ×${ge.reg_factor.toFixed(2)}` : '';
      const seq = ge.seq.split(' ').map((cod) => cod.split('').map((b) => `<span class="${b}">${b}</span>`).join('')).join(' ');
      const isOpen = this.open.has(ge.index);
      return `<div class="gene${isOpen ? ' open' : ''}">
        <div class="gene-top"><span class="gene-idx">#${ge.index}</span><span class="gene-fn${junk ? ' junk' : ''}" title="${esc(fn)}">${esc(fn)}${junk ? '' : ` <span class="muted" style="font-weight:400">${(best.a * 100).toFixed(0)}%</span>`}</span><span class="gene-loc${mem ? ' mem' : ''}">${mem ? 'membrane' : 'cytoplasm'}</span></div>
        ${meter('Expressed', ge.expr / 0.4, (ge.expr * 100).toFixed(1) + '%', mem ? '#ffd27a' : 'var(--accent2)')}
        <div class="gene-meta"><span>${ge.len} codons</span><span>T<sub>opt</sub> ${ge.t_opt.toFixed(0)}°C</span><span>promoter ${ge.promoter.toFixed(2)}${regf}</span>${ge.fold < 0.9 ? `<span>folds ${(ge.fold * 100).toFixed(0)}%</span>` : ''}</div>
        ${side ? `<div class="chips">${side}</div>` : ''}${reg}
        <button class="gene-toggle" data-act="seq" data-gene="${ge.index}">${isOpen ? 'hide sequence' : 'show sequence'}</button>
        <div class="seq">${seq}</div>
      </div>`;
    }).join('');

    const prot = g.genes.map((ge) => [ge.expr, ge]).filter(([e]) => e > 0.001);
    const protStack = `<div style="width:${c.ribo * 100}%;background:#6f7f95" title="Ribosomes (growth machinery)"></div>` +
      prot.map(([e, ge]) => `<div style="width:${e * 100}%;background:${ge.membrane > 0.5 ? '#ffd27a' : '#7fb0ff'};opacity:${0.5 + 0.5 * ((ge.index * 37) % 10) / 10}" title="#${ge.index}"></div>`).join('');

    const chemRows = species.slice(0, 10).map((s, i) => `<tr><td>${s.formula}</td><td>${fmtNum(c.internal[i])}</td><td>${fmtNum(c.external[i])}</td></tr>`).join('') +
      `<tr><td>${species[10].formula}</td><td>${fmtNum(c.internal[10])}</td><td>—</td></tr>`;

    const tdiff = c.temp - c.t_opt;
    const tcol = Math.abs(tdiff) < 10 ? 'var(--accent)' : Math.abs(tdiff) < 20 ? 'var(--warn)' : 'var(--danger)';

    this.el.innerHTML = `
      <div class="insp-head">
        <div class="insp-swatch" style="background:${css(c.color.map((x) => x * 0.35))};border-color:${css(c.color)}"></div>
        <div><div class="insp-title">Cell #${c.id}</div><div class="insp-sub">Generation ${c.generation} · lineage ${c.lineage} · age ${fmtTime(c.age)}</div></div>
        <div class="insp-actions"><button class="btn small ${this.app.follow ? 'toggled' : ''}" data-act="follow" title="Keep the camera on this cell, and jump to a daughter when it divides (F)">Follow</button><button class="btn small" data-act="close" title="Deselect (Esc)">✕</button></div>
      </div>
      <div class="card"><h4>Vital signs</h4>
        ${meter('Size', c.biomass / c.div_size, `${c.biomass.toFixed(1)} µm²`)}
        ${meter('Energy (ATP)', c.atp / Math.max(1e-6, c.atp_cap), fmtNum(c.atp))}
        ${meter('Damage', c.damage, (c.damage * 100).toFixed(0) + '%', 'var(--danger)')}
        ${meter('DNA copied', c.replication, (c.replication * 100).toFixed(0) + '%', '#c9a7ff')}
        ${c.dividing >= 0 ? meter('Dividing', c.dividing, (c.dividing * 100).toFixed(0) + '%', '#ffd27a') : ''}
        <div class="kv" style="margin-top:8px">
          <span class="k">Growth</span><span class="v">${(c.growth * 6000).toFixed(1)}% / min</span>
          <span class="k">Temperature</span><span class="v" style="color:${tcol}">${c.temp.toFixed(1)}°C (best ${c.t_opt.toFixed(0)}°C)</span>
          <span class="k">Enzymes working at</span><span class="v" style="color:${tcol}">${((c.thermal ?? 1) * 100).toFixed(0)}%${(c.thermal ?? 1) < 0.15 ? ' · dormant' : ''}</span>
          <span class="k">Light</span><span class="v">${(c.light * 100).toFixed(0)}%</span>
          <span class="k">Swimming</span><span class="v">${c.swim > 0.05 ? c.swim.toFixed(1) + ' µm/s' : 'no'}</span>
          <span class="k">Radius</span><span class="v">${c.radius.toFixed(2)} µm</span>
        </div>
      </div>
      <div class="card"><h4>Energy sources <span class="hint">${fmtNum(c.atp_rate)} ATP/µm²/s</span></h4>
        ${mix.length ? `<div class="stackbar">${stack}</div>${mixList}` : '<div class="muted">Not producing energy right now.</div>'}
      </div>
      <div class="card"><h4>Metabolism <span class="hint">active reactions</span></h4>${rx}</div>
      <div class="card"><h4>Proteome <span class="hint">ribosomes ${(c.ribo * 100).toFixed(0)}%</span></h4><div class="stackbar">${protStack}</div>
        <div class="muted" style="font-size:11px">Grey: ribosomes (growth). Blue: cytoplasmic proteins. Gold: membrane proteins.</div></div>
      <div class="card"><h4>Genes <span class="hint">${g.genes.length} genes · ${g.codons} codons</span></h4>${genes}</div>
      <div class="card"><h4>Protein shape space <span class="hint">2-D projection</span></h4><canvas class="shape-space"></canvas>
        <div class="muted" style="font-size:11px;margin-top:5px">Each label is a possible function. Dots are this cell's proteins: the closer to a label, the better it performs that function. Mutations move proteins around this space.</div></div>
      <div class="card"><h4>Chemistry <span class="hint">units/µm²</span></h4><table class="chem"><tr><th>Species</th><th>Inside</th><th>Outside</th></tr>${chemRows}</table></div>
      <div class="card"><h4>Universal machinery <span class="hint">inherited from LUCA, evolvable</span></h4><div class="kv">
        <span class="k">Ribosome promoter</span><span class="v">${g.ribo.toFixed(2)}</span>
        <span class="k">Division size</span><span class="v">${g.div_size.toFixed(1)} µm²</span>
        <span class="k">Mutations since LUCA</span><span class="v">${g.mutations}</span>
      </div></div>`;
    this.el.scrollTop = scroll;
    this.drawShapeSpace(g);
  }

  drawShapeSpace(g) {
    const cv = this.el.querySelector('canvas.shape-space');
    const info = this.app.info;
    if (!cv || !info.sites) return;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const W = cv.clientWidth, H = cv.clientHeight;
    cv.width = W * dpr; cv.height = H * dpr;
    const ctx = cv.getContext('2d');
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    const P = this.app.projection();
    const proj = (v) => [P[0].reduce((a, w, k) => a + w * v[k], 0), P[1].reduce((a, w, k) => a + w * v[k], 0)];
    const pts = info.sites.map(proj);
    const xs = pts.map((p) => p[0]), ys = pts.map((p) => p[1]);
    const minx = Math.min(...xs) - 0.4, maxx = Math.max(...xs) + 0.4, miny = Math.min(...ys) - 0.3, maxy = Math.max(...ys) + 0.3;
    const sx = (x) => 8 + ((x - minx) / (maxx - minx)) * (W - 16);
    const sy = (y) => 8 + ((y - miny) / (maxy - miny)) * (H - 16);
    const fcol = (f) => {
      if (f < info.reactions.length) { const s = info.strategies[info.reactions[f].strategy]; return s ? s.color : [160, 170, 190]; }
      return f === info.functions.length - 1 ? [201, 167, 255] : f === info.functions.length - 2 ? [255, 255, 255] : [255, 210, 122];
    };
    ctx.font = '9.5px -apple-system, system-ui, sans-serif';
    const protPts = g.genes.filter((ge) => ge.expr > 0.002).map((ge) => proj(ge.shape));
    const near = (p) => Math.min(9, ...protPts.map((q) => Math.hypot(q[0] - p[0], q[1] - p[1])));
    // Draw every site; label the ones nearest this cell's proteins first,
    // skipping labels that would collide.
    const order = pts.map((p, f) => [near(p), f]).sort((a, b) => a[0] - b[0]);
    const boxes = [];
    pts.forEach((p, f) => {
      const c = fcol(f);
      ctx.fillStyle = `rgba(${c.join(',')},0.18)`;
      ctx.beginPath(); ctx.arc(sx(p[0]), sy(p[1]), 8, 0, Math.PI * 2); ctx.fill();
      ctx.fillStyle = `rgba(${c.join(',')},0.8)`;
      ctx.beginPath(); ctx.arc(sx(p[0]), sy(p[1]), 2, 0, Math.PI * 2); ctx.fill();
    });
    for (const [, f] of order) {
      const p = pts[f], c = fcol(f);
      const name = info.functions[f].replace(' transporter', ' tr.').replace('photosynthesis', 'photo.').replace('Sulfate reduction', 'SO₄ red.').replace(' (thrust)', '');
      const w = ctx.measureText(name).width;
      const bx = sx(p[0]) + 6, by = sy(p[1]) - 6;
      if (boxes.some((b) => bx < b[0] + b[2] && bx + w > b[0] && by < b[1] + 11 && by + 11 > b[1])) continue;
      boxes.push([bx, by, w]);
      ctx.fillStyle = `rgba(${c.join(',')},0.85)`;
      ctx.fillText(name, bx, by + 9);
    }
    for (const ge of g.genes) {
      const p = proj(ge.shape);
      const r = 2.5 + Math.sqrt(ge.expr) * 9;
      ctx.fillStyle = ge.membrane > 0.5 ? 'rgba(255,210,122,0.9)' : 'rgba(127,176,255,0.9)';
      ctx.beginPath(); ctx.arc(sx(p[0]), sy(p[1]), r, 0, Math.PI * 2); ctx.fill();
      ctx.strokeStyle = 'rgba(0,0,0,0.5)'; ctx.stroke();
    }
  }
}
