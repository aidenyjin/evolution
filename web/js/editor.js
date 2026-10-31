// The Editor tab: design new cells gene by gene, or rewrite existing ones.

import { toast } from './ui.js';
import { Renderer } from './renderer.js';

const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const JUNK = 21;

function fmt(v, d = 2) {
  if (!isFinite(v)) return '—';
  const a = Math.abs(v);
  return a >= 100 ? v.toFixed(0) : a >= 10 ? v.toFixed(1) : v.toFixed(d);
}

// Slider <-> value mappings.
const scales = {
  log: (min, max) => ({ to: (v) => (Math.log(v / min) / Math.log(max / min)) * 1000, from: (t) => min * Math.pow(max / min, t / 1000) }),
  lin: (min, max) => ({ to: (v) => ((v - min) / (max - min)) * 1000, from: (t) => min + (t / 1000) * (max - min) }),
  sq: (min, max) => ({ to: (v) => Math.sqrt(Math.max(0, v - min) / (max - min)) * 1000, from: (t) => min + (t / 1000) ** 2 * (max - min) }),
};

export class Editor {
  constructor(el, app) {
    this.el = el;
    this.app = app;
    this.spec = null;
    this.preview = [];
    this.mode = 'design';
    this.cellId = null;
    this.dirtyState = new Set();
    this.dirtyGenome = false;
    this.openGenes = new Set();
    this.redesign = new Set();
    this.foldTimer = null;
    this.count = 10;
    this.spread = 10;
    this.pauseWhileEditing = true;
    this.pausedByEditor = false;
    this.addOpts = { f: 11, t_opt: 30, len: 50, loc: 'auto', promoter: 0.2 };
    this.el.addEventListener('input', (e) => this.onInput(e));
    this.el.addEventListener('change', (e) => this.onChange(e));
    this.el.addEventListener('click', (e) => this.onClick(e));
  }

  async load(kind) {
    let r;
    try {
      if (kind === 'luca' || kind === 'blank') r = await this.app.request({ cmd: 'template', name: kind });
      else r = await this.app.request({ cmd: 'cell_spec', id: this.app.selected });
    } catch { return; }
    if (r.type !== 'spec') { toast(r.text || 'Could not load that cell', 'error'); return; }
    this.endEditPause();
    this.spec = r.spec;
    this.dirtyState.clear();
    this.dirtyGenome = false;
    this.openGenes.clear();
    this.redesign.clear();
    if (kind === 'edit') {
      this.mode = 'edit';
      this.cellId = r.cell.id;
      this.cellInfo = r.cell;
      if (this.pauseWhileEditing && !this.app.paused) { this.app.togglePause(); this.pausedByEditor = true; }
    } else {
      this.mode = 'design';
      this.cellId = null;
      this.source = kind === 'copy' ? `a copy of cell #${r.cell.id}` : kind === 'luca' ? 'the ancestor (LUCA)' : 'a blank cell';
      if (kind === 'copy') {
        // A copy starts healthy, at a modest size, as a new lineage.
        const st = this.spec.state;
        st.biomass = this.spec.genome.div_size * 0.7; st.damage = 0; st.replication = 0; st.atp_frac = Math.max(st.atp_frac, 0.4);
        this.spec.genome.hue = (this.spec.genome.hue + 0.5) % 1;
      }
    }
    this.app.stopPlacing();
    this.render();
    this.refold();
  }

  endEditPause() {
    if (this.pausedByEditor && this.app.paused) this.app.togglePause();
    this.pausedByEditor = false;
  }

  // ------------------------------------------------------------- previews

  refold(delay = 0) {
    clearTimeout(this.foldTimer);
    this.foldTimer = setTimeout(async () => {
      if (!this.spec) return;
      const seqs = this.spec.genome.genes.map((g) => g.codons);
      try {
        const r = await this.app.request({ cmd: 'fold', seqs });
        this.preview = r.proteins.map((p) => p.protein);
        this.preview.forEach((_, i) => this.renderGenePreview(i));
        this.renderSummary();
      } catch { /* ignore */ }
    }, delay);
  }

  shares() {
    const g = this.spec.genome;
    const z = g.ribo + g.genes.reduce((a, x) => a + x.promoter, 0);
    return { ribo: g.ribo / z, genes: g.genes.map((x) => x.promoter / z) };
  }

  fnLabel(p) {
    if (!p) return ['…', true, 0];
    const best = p.acts[0];
    if (!best || best.a < 0.1) return ['No clear function', true, best ? best.a : 0];
    return [best.name, false, best.a];
  }

  // ------------------------------------------------------------- rendering

  slider(id, label, value, min, max, scale, unit, hint = '') {
    const sc = scales[scale](min, max);
    return `<div class="param"><div class="param-top"><span class="param-label">${label}</span>
      <input class="param-num" type="number" step="any" data-num="${id}" value="${fmt(value, 3)}"><span class="param-unit">${unit}</span></div>
      <input type="range" min="0" max="1000" step="1" data-range="${id}" data-scale="${scale}" data-min="${min}" data-max="${max}" value="${sc.to(value)}">
      ${hint ? `<div class="param-desc">${hint}</div>` : ''}</div>`;
  }

  fnOptions(selected) {
    const fns = this.app.info?.functions || [];
    return fns.map((n, i) => `<option value="${i}" ${i === selected ? 'selected' : ''}>${esc(n)}</option>`).join('') +
      `<option value="${JUNK}" ${selected === JUNK ? 'selected' : ''}>Random DNA (no designed function)</option>`;
  }

  geneCard(g, i) {
    const open = this.openGenes.has(i);
    const rd = this.redesign.has(i);
    return `<div class="gene ed-gene ${open ? 'open' : ''}" data-gene="${i}">
      <div class="gene-top"><span class="gene-idx">#${i}</span><span class="gene-fn" data-fn="${i}">…</span><span class="gene-loc" data-loc="${i}"></span></div>
      ${this.slider(`g${i}.promoter`, 'Expression (promoter)', g.promoter, 0, 10, 'sq', '', '')}
      <div class="gene-meta" data-meta="${i}"></div>
      <div class="chips" data-chips="${i}"></div>
      <div class="ed-gene-actions">
        <button class="gene-toggle" data-act="toggle" data-i="${i}">${open ? 'hide DNA' : 'edit DNA'}</button>
        <button class="gene-toggle" data-act="redesign" data-i="${i}">${rd ? 'cancel redesign' : 'redesign…'}</button>
        <button class="gene-toggle" data-act="dup" data-i="${i}">duplicate</button>
        <button class="gene-toggle danger" data-act="remove" data-i="${i}">remove</button>
      </div>
      ${rd ? `<div class="ed-redesign">${this.designControls(`rd${i}`, this.addOpts)}<button class="btn small primary" data-act="do-redesign" data-i="${i}">Design new DNA</button></div>` : ''}
      <div class="ed-dna">
        <textarea class="ed-codons" data-codons="${i}" spellcheck="false" rows="4">${esc(g.codons)}</textarea>
        <div class="param-desc" data-codon-info="${i}"></div>
        <div class="ed-op">Regulator binding site <input class="param-num" type="number" step="0.1" data-op="${i}.0" value="${fmt(g.operator?.[0] ?? 0)}"><input class="param-num" type="number" step="0.1" data-op="${i}.1" value="${fmt(g.operator?.[1] ?? 0)}"></div>
      </div>
    </div>`;
  }

  designControls(prefix, o) {
    return `<div class="ed-design">
      <label>Function<select data-d="${prefix}.f">${this.fnOptions(o.f)}</select></label>
      <label>Best temperature <span data-dv="${prefix}.t_opt">${o.t_opt} °C</span><input type="range" min="0" max="90" step="1" value="${o.t_opt}" data-d="${prefix}.t_opt"></label>
      <label>Length <span data-dv="${prefix}.len">${o.len} codons</span><input type="range" min="20" max="200" step="1" value="${o.len}" data-d="${prefix}.len"></label>
      <label>Location<select data-d="${prefix}.loc"><option value="auto" ${o.loc === 'auto' ? 'selected' : ''}>Automatic</option><option value="cytoplasm" ${o.loc === 'cytoplasm' ? 'selected' : ''}>Cytoplasm</option><option value="membrane" ${o.loc === 'membrane' ? 'selected' : ''}>Membrane</option></select></label>
    </div>`;
  }

  render() {
    if (!this.spec) {
      this.el.innerHTML = `<div class="ed-intro">
        <h4>Cell editor</h4>
        <p class="muted">Design cells gene by gene and place them in the world, or rewrite a living cell. A cell's traits come from its genes, so you edit genes and the simulation works out what each protein does.</p>
        <div class="ed-start">
          <button class="btn" data-act="luca">Start from LUCA</button>
          <button class="btn" data-act="blank">Start blank</button>
          <button class="btn" data-act="copy" ${this.app.selected == null ? 'disabled' : ''}>Copy selected cell</button>
          <button class="btn primary" data-act="edit" ${this.app.selected == null ? 'disabled' : ''}>Edit selected cell</button>
        </div>
        <p class="muted" style="font-size:11.5px">Select a cell by clicking it in the world to copy or edit it.</p></div>`;
      return;
    }
    const g = this.spec.genome, st = this.spec.state;
    const hueCol = Renderer.lineageColor(g.hue).map((x) => Math.round(x * 255)).join(',');
    const head = this.mode === 'edit'
      ? `<div class="ed-mode edit">Editing <b>cell #${this.cellId}</b> · generation ${this.cellInfo?.generation ?? '?'}${this.app.paused ? ' · simulation paused' : ''}</div>`
      : `<div class="ed-mode">Designing a new cell from ${esc(this.source || 'scratch')}</div>`;
    const actions = this.mode === 'edit'
      ? `<div class="card ed-actions"><div class="ed-buttons">
          <button class="btn primary" data-act="apply">Apply to cell #${this.cellId}</button>
          <button class="btn" data-act="revert">Revert</button>
          <button class="btn danger" data-act="delete">Remove cell</button></div>
          <label class="toggle"><input type="checkbox" data-act="pausetoggle" ${this.pauseWhileEditing ? 'checked' : ''}><span>Pause the simulation while editing</span></label></div>`
      : `<div class="card ed-actions">
          <div class="ed-place-row"><label>Cells <input class="param-num" type="number" min="1" max="500" data-place="count" value="${this.count}"></label>
          <label>Spread <input class="param-num" type="number" min="0" max="300" data-place="spread" value="${this.spread}"> µm</label></div>
          <button class="btn primary ${this.app.placing ? 'toggled' : ''}" data-act="place">${this.app.placing ? 'Placing… click in the world (Esc to stop)' : 'Place in the world'}</button></div>`;
    const sh = this.shares();
    const prot = `<div class="stackbar"><div style="width:${sh.ribo * 100}%;background:#6f7f95" title="Ribosomes"></div>${sh.genes.map((s, i) => `<div style="width:${s * 100}%;background:${i % 2 ? '#7fb0ff' : '#a8c8ff'}" title="#${i}"></div>`).join('')}</div>`;
    const sp = this.app.info?.species || [];
    const chemRows = sp.map((s, k) => `<label class="ed-chem">${s.formula}<input class="param-num" type="number" step="any" min="0" data-chem="${k}" value="${fmt(st.internal?.[k] ?? 0, 3)}"></label>`).join('');

    this.el.innerHTML = `
      <div class="ed-top">${head}<button class="btn small" data-act="close-editor" title="Close the editor">✕</button></div>
      ${actions}
      <div class="card"><h4>Cell state</h4>
        ${this.slider('state.biomass', 'Size', st.biomass, 4, 600, 'log', 'µm²', 'Biomass equals area. The cell divides when it reaches its division size.')}
        ${this.slider('state.atp_frac', 'Energy (ATP)', st.atp_frac * 100, 0, 100, 'lin', '%', '')}
        ${this.slider('state.damage', 'Damage', st.damage * 100, 0, 99, 'lin', '%', '')}
        ${this.slider('state.replication', 'DNA copied', st.replication * 100, 0, 100, 'lin', '%', '')}
      </div>
      <div class="card"><h4>Universal machinery</h4>
        ${this.slider('genome.ribo', 'Ribosome promoter', g.ribo, 0.05, 20, 'log', '', 'More ribosomes: faster growth, less room for other proteins.')}
        ${this.slider('genome.div_size', 'Division size', g.div_size, 6, 600, 'log', 'µm²', '')}
        <div class="param"><div class="param-top"><span class="param-label">Lineage colour</span><span class="ed-swatch" style="background:rgb(${hueCol})"></span></div>
        <input type="range" min="0" max="1000" data-hue="1" value="${g.hue * 1000}"></div>
      </div>
      <div class="card"><h4>Genes <span class="hint" data-summary="1"></span></h4>
        ${prot}
        <div class="muted" style="font-size:11px;margin-bottom:4px">Grey: ribosomes. Blue: this cell's proteins, as shares of everything it makes.</div>
        ${g.genes.map((x, i) => this.geneCard(x, i)).join('') || '<div class="muted">No genes yet. A cell with no genes can do nothing; add some below.</div>'}
      </div>
      <div class="card"><h4>Add a gene</h4>
        <p class="muted" style="font-size:11.5px;margin:0 0 6px">Pick what you want the protein to do. The simulator searches for DNA whose protein folds into that function, with weak side activities like real enzymes.</p>
        ${this.designControls('add', this.addOpts)}
        ${this.slider('add.promoter', 'Expression (promoter)', this.addOpts.promoter, 0, 10, 'sq', '', '')}
        <button class="btn primary" data-act="add-gene">Design &amp; add gene</button>
      </div>
      <details class="card ed-more"><summary>Internal chemistry <span class="hint">units/µm²</span></summary><div class="ed-chem-grid">${chemRows}</div></details>`;
    this.preview.forEach((_, i) => this.renderGenePreview(i));
    this.renderSummary();
  }

  renderSummary() {
    const el = this.el.querySelector('[data-summary]');
    if (!el || !this.spec) return;
    const n = this.spec.genome.genes.length;
    const codons = this.preview.reduce((a, p) => a + (p?.len || 0), 0);
    const speed = this.app.params?.replication_speed || 30;
    el.textContent = `${n} gene${n === 1 ? '' : 's'} · ${codons} codons · DNA copy ≈ ${(codons / speed).toFixed(0)} s`;
  }

  renderGenePreview(i) {
    const p = this.preview[i];
    const fnEl = this.el.querySelector(`[data-fn="${i}"]`);
    if (!fnEl || !p) return;
    const [name, junk, a] = this.fnLabel(p);
    fnEl.innerHTML = `${esc(name)}${junk ? '' : ` <span class="muted" style="font-weight:400">${(a * 100).toFixed(0)}%</span>`}`;
    fnEl.classList.toggle('junk', junk);
    const loc = this.el.querySelector(`[data-loc="${i}"]`);
    const mem = p.membrane > 0.5;
    loc.textContent = mem ? 'membrane' : 'cytoplasm';
    loc.classList.toggle('mem', mem);
    const share = this.shares().genes[i] || 0;
    this.el.querySelector(`[data-meta="${i}"]`).innerHTML = `<span>${p.len} codons</span><span>T<sub>opt</sub> ${p.t_opt.toFixed(0)}°C</span><span>${(share * 100).toFixed(1)}% of proteome</span>${p.fold < 0.9 ? `<span>folds ${(p.fold * 100).toFixed(0)}%</span>` : ''}`;
    const side = p.acts.slice(junk ? 0 : 1, 5).filter((x) => x.a > 0.02).map((x) => `<span class="chip">${esc(x.name)} <b>${(x.a * 100).toFixed(0)}%</b></span>`).join('');
    const reg = p.reg ? `<span class="chip" style="color:#c9a7ff">regulator: senses ${esc(p.reg.ligand)}, ${p.reg.activator ? 'activates' : 'represses'} genes ${p.reg.ligand_activates ? 'when present' : 'when absent'}</span>` : '';
    this.el.querySelector(`[data-chips="${i}"]`).innerHTML = side + reg;
    const info = this.el.querySelector(`[data-codon-info="${i}"]`);
    if (info) {
      const raw = this.spec.genome.genes[i].codons.replace(/[^ACGUTacgut]/g, '');
      const gc = raw.length ? (raw.match(/[GCgc]/g) || []).length / raw.length : 0;
      info.textContent = `${Math.floor(raw.length / 3)} codons${raw.length % 3 ? ` (+${raw.length % 3} stray bases ignored)` : ''} · ${(gc * 100).toFixed(0)}% G/C. Letters U/T, C, A, G; spaces don't matter. G/C at the 3rd position raises heat tolerance; the 2nd base U makes proteins stick in the membrane.`;
    }
  }

  // ------------------------------------------------------------- input

  setValue(id, v) {
    const [scope, key] = id.split('.');
    const st = this.spec.state, g = this.spec.genome;
    if (scope === 'state') {
      st[key] = key === 'biomass' ? v : v / 100;
      this.dirtyState.add(key);
    } else if (scope === 'genome') {
      g[key] = v; this.dirtyGenome = true;
    } else if (scope === 'add') {
      this.addOpts[key] = v;
    } else if (scope.startsWith('g')) {
      g.genes[Number(scope.slice(1))][key] = v; this.dirtyGenome = true;
      this.preview.forEach((_, i) => this.renderGenePreview(i));
    }
  }

  onInput(e) {
    const t = e.target;
    if (t.dataset.range) {
      const sc = scales[t.dataset.scale](Number(t.dataset.min), Number(t.dataset.max));
      const v = sc.from(Number(t.value));
      this.setValue(t.dataset.range, v);
      const num = this.el.querySelector(`[data-num="${t.dataset.range}"]`);
      if (num) num.value = fmt(v, 3);
      if (t.dataset.range.endsWith('promoter')) this.updateProteomeBar();
    } else if (t.dataset.hue) {
      this.spec.genome.hue = Number(t.value) / 1000;
      this.dirtyGenome = true;
      const c = Renderer.lineageColor(this.spec.genome.hue).map((x) => Math.round(x * 255)).join(',');
      this.el.querySelector('.ed-swatch').style.background = `rgb(${c})`;
    } else if (t.dataset.codons !== undefined) {
      this.spec.genome.genes[Number(t.dataset.codons)].codons = t.value;
      this.dirtyGenome = true;
      this.refold(250);
    } else if (t.dataset.d) {
      const [prefix, key] = t.dataset.d.split('.');
      if (key === 't_opt' || key === 'len') {
        this.addOpts[key] = Number(t.value);
        const lab = this.el.querySelector(`[data-dv="${t.dataset.d}"]`);
        if (lab) lab.textContent = key === 't_opt' ? `${t.value} °C` : `${t.value} codons`;
      }
      void prefix;
    }
  }

  onChange(e) {
    const t = e.target;
    if (t.dataset.num) {
      const x = parseFloat(t.value);
      if (!isFinite(x)) return;
      this.setValue(t.dataset.num, x);
      const rng = this.el.querySelector(`[data-range="${t.dataset.num}"]`);
      if (rng) rng.value = scales[rng.dataset.scale](Number(rng.dataset.min), Number(rng.dataset.max)).to(x);
      if (t.dataset.num.endsWith('promoter')) this.updateProteomeBar();
    } else if (t.dataset.d) {
      const key = t.dataset.d.split('.')[1];
      if (key === 'f') this.addOpts.f = Number(t.value);
      if (key === 'loc') this.addOpts.loc = t.value;
    } else if (t.dataset.op) {
      const [i, k] = t.dataset.op.split('.').map(Number);
      const gene = this.spec.genome.genes[i];
      gene.operator = gene.operator || [0, 0];
      gene.operator[k] = parseFloat(t.value) || 0;
      this.dirtyGenome = true;
    } else if (t.dataset.chem !== undefined) {
      this.spec.state.internal[Number(t.dataset.chem)] = Math.max(0, parseFloat(t.value) || 0);
      this.dirtyState.add('internal');
    } else if (t.dataset.place) {
      const v = Math.max(0, parseFloat(t.value) || 0);
      if (t.dataset.place === 'count') this.count = Math.max(1, Math.min(500, Math.round(v)));
      else this.spread = Math.min(300, v);
      if (this.app.placing) this.app.startPlacing(this.spec, this.count, this.spread);
    } else if (t.dataset.act === 'pausetoggle') {
      this.pauseWhileEditing = t.checked;
    }
  }

  updateProteomeBar() {
    const bar = this.el.querySelector('.card .stackbar');
    if (!bar) return;
    const sh = this.shares();
    const kids = bar.children;
    if (kids.length === sh.genes.length + 1) {
      kids[0].style.width = `${sh.ribo * 100}%`;
      sh.genes.forEach((s, i) => (kids[i + 1].style.width = `${s * 100}%`));
    }
    this.preview.forEach((_, i) => this.renderGenePreview(i));
  }

  async designGene() {
    const o = this.addOpts;
    return this.app.request({ cmd: 'design', f: o.f, t_opt: o.t_opt, len: o.len, loc: o.loc });
  }

  async onClick(e) {
    const t = e.target.closest('[data-act]');
    if (!t || t.tagName === 'INPUT') return;
    const act = t.dataset.act, i = Number(t.dataset.i);
    const genes = this.spec?.genome.genes;
    switch (act) {
      case 'luca': case 'blank': case 'copy': case 'edit': return this.load(act);
      case 'close-editor':
        this.endEditPause(); this.app.stopPlacing(); this.spec = null; this.preview = []; return this.render();
      case 'toggle':
        if (this.openGenes.has(i)) this.openGenes.delete(i); else this.openGenes.add(i);
        t.closest('.gene').classList.toggle('open');
        t.textContent = this.openGenes.has(i) ? 'hide DNA' : 'edit DNA';
        return;
      case 'redesign':
        if (this.redesign.has(i)) this.redesign.delete(i); else this.redesign.add(i);
        return this.render();
      case 'do-redesign': {
        t.disabled = true; t.textContent = 'Designing…';
        try {
          const r = await this.designGene();
          genes[i].codons = r.codons;
          this.preview[i] = r.protein;
          this.redesign.delete(i);
          this.dirtyGenome = true;
        } catch { toast('Design failed', 'error'); }
        this.render();
        return;
      }
      case 'dup':
        genes.splice(i + 1, 0, JSON.parse(JSON.stringify(genes[i])));
        this.preview.splice(i + 1, 0, this.preview[i]);
        this.dirtyGenome = true; this.openGenes.clear(); this.redesign.clear();
        return this.render();
      case 'remove':
        genes.splice(i, 1); this.preview.splice(i, 1);
        this.dirtyGenome = true; this.openGenes.clear(); this.redesign.clear();
        return this.render();
      case 'add-gene': {
        if (genes.length >= 256) { toast('A genome can hold at most 256 genes', 'error'); return; }
        t.disabled = true; t.textContent = 'Designing…';
        try {
          const r = await this.designGene();
          genes.push({ codons: r.codons, promoter: this.addOpts.promoter, operator: [Math.random() * 2 - 1, Math.random() * 2 - 1] });
          this.preview.push(r.protein);
          this.dirtyGenome = true;
          toast(`Added: ${this.fnLabel(r.protein)[0]}`);
        } catch { toast('Design failed', 'error'); }
        this.render();
        return;
      }
      case 'place':
        if (this.app.placing) this.app.stopPlacing();
        else this.app.startPlacing(this.spec, this.count, this.spread);
        return this.render();
      case 'apply': {
        const spec = {};
        if (this.dirtyGenome) spec.genome = this.spec.genome;
        if (this.dirtyState.size) {
          spec.state = {};
          for (const k of this.dirtyState) spec.state[k] = this.spec.state[k];
        }
        if (!spec.genome && !spec.state) { toast('Nothing changed yet'); return; }
        const r = await this.app.request({ cmd: 'edit_cell', id: this.cellId, spec });
        toast(r.text, r.ok ? '' : 'error');
        if (r.ok) { this.dirtyGenome = false; this.dirtyState.clear(); this.endEditPause(); this.render(); }
        return;
      }
      case 'revert': return this.load('edit');
      case 'delete': {
        const r = await this.app.request({ cmd: 'delete_cell', id: this.cellId });
        toast(r.text, r.ok ? '' : 'error');
        this.endEditPause();
        this.spec = null; this.app.select(null); this.render();
        return;
      }
    }
  }

  /// Called when the selected cell changes, to refresh the start buttons.
  selectionChanged() { if (!this.spec) this.render(); }
}
