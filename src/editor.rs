//! God tools: design new cells from scratch, or rewrite existing ones.
//!
//! A cell is described by a JSON "spec": its genome (every gene's codons,
//! promoter strength and regulator binding site, plus the universal
//! machinery settings) and optionally its current state. Traits are not
//! edited directly; they follow from the genes, exactly as for evolved cells.

use crate::cell::Cell;
use crate::chem::{self, NF, NS_CELL};
use crate::genome::{self, Gene, Genome, Protein, N_LIGANDS};
use crate::params::Params;
use crate::rng::Rng;
use crate::stats::CAP_THRESHOLD;
use crate::world::{grid_index, World};
use serde_json::{json, Value};
use std::sync::Arc;

/// Pseudo-function index meaning "random DNA with no designed purpose".
pub const JUNK: usize = NF;
const MAX_CODONS: usize = 400;
const MAX_GENES: usize = 256;

/// Parse a codon string. Accepts U or T, ignores spaces and anything else;
/// a trailing incomplete codon is dropped.
pub fn parse_codons(s: &str) -> Vec<u8> {
    let bases: Vec<u8> = s
        .chars()
        .filter_map(|c| match c.to_ascii_uppercase() {
            'U' | 'T' => Some(0),
            'C' => Some(1),
            'A' => Some(2),
            'G' => Some(3),
            _ => None,
        })
        .collect();
    bases.chunks_exact(3).map(|b| (b[0] << 4) | (b[1] << 2) | b[2]).take(MAX_CODONS).collect()
}

pub fn codons_string(c: &[u8]) -> String {
    c.iter().map(|&x| genome::codon_str(x)).collect::<Vec<_>>().join(" ")
}

/// What a protein does, for previews.
pub fn protein_json(p: &Protein) -> Value {
    let mut acts = p.acts.clone();
    acts.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let acts: Vec<Value> = acts.iter().map(|(f, a)| json!({"f": f, "name": chem::function_name(*f as usize), "a": a})).collect();
    let reg = p.reg.as_ref().map(|r| {
        let lig = r.ligand as usize;
        let ligand = if lig < NS_CELL { chem::SPECIES[lig].formula.to_string() } else { "ATP".to_string() };
        let _ = N_LIGANDS;
        json!({"ligand": ligand, "activator": r.activator, "ligand_activates": r.ligand_activates, "op": r.op})
    });
    json!({"len": p.len, "membrane": p.membrane, "fold": p.fold, "t_opt": p.t_opt, "shape": p.shape, "acts": acts, "reg": reg})
}

pub fn fold_many(seqs: &[Value]) -> Value {
    let proteins: Vec<Value> = seqs
        .iter()
        .map(|s| {
            let codons = parse_codons(s.as_str().unwrap_or(""));
            json!({"codons": codons.len(), "protein": protein_json(&genome::fold(&codons))})
        })
        .collect();
    json!({"type": "fold", "proteins": proteins})
}

/// Design a gene whose protein performs function `f` (or random junk).
pub fn design(f: usize, t_opt: f32, len: usize, loc: &str, seed: u64) -> Value {
    let mut rng = Rng::new(seed);
    let len = len.clamp(10, MAX_CODONS);
    let codons: Vec<u8> = if f >= JUNK {
        (0..len).map(|_| rng.below(64) as u8).collect()
    } else {
        let membrane = match loc {
            "membrane" => true,
            "cytoplasm" => false,
            _ => (chem::F_TRANSPORT0..=chem::F_MOTOR).contains(&f),
        };
        genome::design_gene(f, membrane, t_opt.clamp(0.0, 90.0), len, &mut rng)
    };
    json!({"type": "design", "codons": codons_string(&codons), "protein": protein_json(&genome::fold(&codons))})
}

fn genome_spec(g: &Genome) -> Value {
    let genes: Vec<Value> = g
        .genes
        .iter()
        .map(|gene| json!({"codons": codons_string(&gene.codons), "promoter": gene.promoter, "operator": gene.operator}))
        .collect();
    json!({"ribo": g.ribo, "div_size": g.div_size, "hue": g.hue, "mutations": g.mutations, "genes": genes})
}

/// Default state for a newly placed cell.
fn default_state(g: &Genome) -> Value {
    let mut internal = vec![0.0f32; NS_CELL];
    internal[chem::SUGAR] = 0.4;
    internal[chem::NH4] = 0.1;
    internal[chem::PO4] = 0.02;
    json!({"biomass": g.div_size * 0.7, "atp_frac": 0.4, "damage": 0.0, "replication": 0.0, "internal": internal})
}

pub fn template(name: &str) -> Value {
    let g = match name {
        "blank" => {
            let mut g = Genome { genes: Vec::new(), ribo: 1.0, div_size: 36.0, hue: 0.55, total_codons: 0, has_regulators: false, mutations: 0 };
            g.finish();
            g
        }
        _ => {
            let mut g = genome::luca(&mut Rng::new(0x1_0CA));
            g.hue = 0.55;
            g
        }
    };
    json!({"type": "spec", "spec": {"genome": genome_spec(&g), "state": default_state(&g)}, "source": name})
}

pub fn cell_spec(w: &World, id: u64) -> Value {
    let Some(c) = w.cells.iter().find(|c| c.id as u32 == id as u32) else {
        return json!({"type": "toast", "text": "That cell no longer exists (it divided or died).", "ok": false});
    };
    let area = c.biomass.max(1e-3);
    let internal: Vec<f32> = (0..NS_CELL).map(|s| c.amt[s] / area).collect();
    let cap = w.params.atp_capacity * c.biomass;
    json!({
        "type": "spec",
        "source": "cell",
        "cell": {"id": c.id as u32, "generation": c.generation, "lineage": c.lineage},
        "spec": {
            "genome": genome_spec(&c.genome),
            "state": {
                "biomass": c.biomass, "atp_frac": if cap > 0.0 { c.atp / cap } else { 0.0 },
                "damage": c.damage, "replication": c.replication, "internal": internal,
            },
        },
    })
}

fn num(v: &Value, key: &str) -> Option<f32> {
    v.get(key).and_then(|x| x.as_f64()).map(|x| x as f32).filter(|x| x.is_finite())
}

fn genome_from_spec(v: &Value, mutations: u32, rng: &mut Rng) -> Result<Genome, String> {
    let genes_v = v.get("genes").and_then(|g| g.as_array()).ok_or("genome has no gene list")?;
    if genes_v.len() > MAX_GENES {
        return Err(format!("too many genes (max {MAX_GENES})"));
    }
    let mut genes = Vec::with_capacity(genes_v.len());
    for (i, gv) in genes_v.iter().enumerate() {
        let codons = parse_codons(gv.get("codons").and_then(|c| c.as_str()).unwrap_or(""));
        if codons.len() < 3 {
            return Err(format!("gene #{i} needs at least 3 codons"));
        }
        let promoter = num(gv, "promoter").unwrap_or(0.1).clamp(0.0, 10.0);
        let operator = gv
            .get("operator")
            .and_then(|o| o.as_array())
            .and_then(|o| Some([o.first()?.as_f64()? as f32, o.get(1)?.as_f64()? as f32]))
            .unwrap_or_else(|| [rng.normal(), rng.normal()]);
        genes.push(Gene::new(codons, promoter, operator));
    }
    let mut g = Genome {
        genes,
        ribo: num(v, "ribo").unwrap_or(1.0).clamp(0.05, 20.0),
        div_size: num(v, "div_size").unwrap_or(36.0).clamp(6.0, 600.0),
        hue: num(v, "hue").unwrap_or(0.55).rem_euclid(1.0),
        total_codons: 0,
        has_regulators: false,
        mutations,
    };
    g.finish();
    Ok(g)
}

/// Apply whichever state fields the spec contains.
fn apply_state(c: &mut Cell, s: &Value, p: &Params) {
    if let Some(b) = num(s, "biomass") {
        // Keep internal concentrations when resizing.
        let old = c.biomass.max(1e-3);
        let b = b.clamp(4.0, 2000.0);
        for a in c.amt.iter_mut() {
            *a *= b / old;
        }
        c.atp *= b / old;
        c.biomass = b;
        c.radius = (b / std::f32::consts::PI).sqrt();
    }
    if let Some(f) = num(s, "atp_frac") {
        c.atp = f.clamp(0.0, 1.0) * p.atp_capacity * c.biomass;
    }
    if let Some(d) = num(s, "damage") {
        c.damage = d.clamp(0.0, 0.99);
    }
    if let Some(r) = num(s, "replication") {
        c.replication = r.clamp(0.0, 1.0);
    }
    if let Some(arr) = s.get("internal").and_then(|a| a.as_array()) {
        for (k, v) in arr.iter().enumerate().take(NS_CELL) {
            if let Some(x) = v.as_f64() {
                c.amt[k] = (x as f32).clamp(0.0, 1e4) * c.biomass;
            }
        }
    }
}

impl World {
    /// Silently record that functions present in a hand-made cell are known,
    /// so evolution isn't later credited with "discovering" them.
    fn mark_designed(&mut self, c: &Cell) {
        for f in 0..NF {
            if c.cap[f] > CAP_THRESHOLD * 0.25 {
                self.stats.discovered[f] = true;
                if f < chem::NR {
                    self.stats.strat_discovered[chem::REACTIONS[f].strategy as usize] = true;
                }
            }
        }
    }

    pub fn edit_cell(&mut self, id: u64, spec: &Value) -> Result<String, String> {
        let idx = self.cells.iter().position(|c| c.id as u32 == id as u32).ok_or("That cell no longer exists (it divided or died).")?;
        let mut rng = self.rng.fork();
        let new_genome = match spec.get("genome") {
            Some(g) if !g.is_null() => Some(genome_from_spec(g, self.cells[idx].genome.mutations, &mut rng)?),
            _ => None,
        };
        let p = self.params.clone();
        let temp = self.temp_at(self.cells[idx].gi as usize);
        let c = &mut self.cells[idx];
        if let Some(g) = new_genome {
            c.genome = Arc::new(g);
            c.set_expression_to_target();
            c.replication = c.replication.min(1.0);
        }
        if let Some(s) = spec.get("state") {
            apply_state(c, s, &p);
        }
        c.update_capabilities(temp, &p);
        let c = c.clone();
        self.mark_designed(&c);
        let text = format!("You edited cell #{} ({} genes)", c.id, c.genome.genes.len());
        self.stats.push_event(self.time, self.step, "intervention", text.clone(), Some(c.id), c.x, c.y);
        Ok(text)
    }

    pub fn spawn_cells(&mut self, spec: &Value, x: f32, y: f32, count: usize, spread: f32) -> Result<String, String> {
        let count = count.clamp(1, 500);
        if (self.cells.len() + count) as f32 > self.params.max_population {
            return Err("That would exceed the population safety cap (Settings → Growth & division).".into());
        }
        let mut rng = self.rng.fork();
        let g = Arc::new(genome_from_spec(spec.get("genome").ok_or("spec has no genome")?, 0, &mut rng)?);
        let state = spec.get("state").cloned().unwrap_or_else(|| default_state(&g));
        let p = self.params.clone();
        let lineage = self.next_id as u32;
        let spread = spread.clamp(0.0, 300.0);
        for _ in 0..count {
            let a = rng.range(0.0, std::f32::consts::TAU);
            let d = spread * rng.f32().sqrt();
            let cx = (x + a.cos() * d).clamp(3.0, self.w - 3.0);
            let cy = (y + a.sin() * d).clamp(3.0, self.h - 3.0);
            let id = self.next_id;
            self.next_id += 1;
            let mut c = Cell::new(id, lineage, cx, cy, g.div_size * 0.7, g.clone(), rng.fork(), self.time);
            c.amt = [0.0; NS_CELL];
            apply_state(&mut c, &state, &p);
            c.gi = grid_index(c.x, c.y, self.dx, self.gw, self.gh);
            let temp = self.temp_at(c.gi as usize);
            c.update_capabilities(temp, &p);
            self.mark_designed(&c);
            self.cells.push(c);
        }
        let text = format!("You added {} custom cell{} (new lineage #{})", count, if count == 1 { "" } else { "s" }, lineage);
        self.stats.push_event(self.time, self.step, "intervention", text.clone(), None, x, y);
        Ok(text)
    }

    pub fn delete_cell(&mut self, id: u64) -> Result<String, String> {
        let idx = self.cells.iter().position(|c| c.id as u32 == id as u32).ok_or("That cell no longer exists.")?;
        let c = self.cells.swap_remove(idx);
        Ok(format!("Removed cell #{}", c.id))
    }
}
