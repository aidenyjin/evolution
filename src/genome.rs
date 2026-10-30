//! Genes, the genetic code, protein folding and mutation.
//!
//! A gene is a string of codons (3 bases each, 64 possibilities). A fixed
//! "folding" rule turns the codon sequence into a protein with physical
//! properties: a shape (which decides what it binds and catalyses), how
//! hydrophobic it is (membrane or cytoplasm), whether it is long enough to
//! fold, and its temperature optimum. Nothing about what a protein *is for*
//! is written anywhere; function falls out of shape.
//!
//! Like the real genetic code, the third base of a codon matters least
//! ("wobble"), the second base mostly decides hydrophobicity, and G/C at the
//! third position makes proteins more heat-stable.

use crate::chem::{self, NF, NS_CELL, SHAPE_DIM, SITE_SIGMA};
use crate::params::Params;
use crate::rng::Rng;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

pub const BASES: [char; 4] = ['U', 'C', 'A', 'G'];
const SEC_DIM: usize = 6;
/// Ligands a regulator can sense: every internal species plus ATP.
pub const N_LIGANDS: usize = NS_CELL + 1;
pub const LIGAND_ATP: usize = NS_CELL;

pub struct Universe {
    shape_tab: [[[f32; SHAPE_DIM]; 4]; 3],
    sec_tab: [[[f32; SEC_DIM]; 4]; 3],
    hyd: [f32; 64],
    gc: [f32; 64],
    pub sites: [[f32; SHAPE_DIM]; NF],
}

pub fn universe() -> &'static Universe {
    static U: OnceLock<Universe> = OnceLock::new();
    U.get_or_init(|| {
        let mut rng = Rng::new(0xC0D0_7AB1E);
        let mut shape_tab = [[[0.0; SHAPE_DIM]; 4]; 3];
        let mut sec_tab = [[[0.0; SEC_DIM]; 4]; 3];
        // Random per-base vectors, centred and scaled to unit variance per
        // component so random proteins have shapes ~ N(0, 1).
        fn fill<const D: usize>(t: &mut [[f32; D]; 4], rng: &mut Rng) {
            for k in 0..D {
                let mut v = [0.0f32; 4];
                for b in 0..4 {
                    v[b] = rng.normal();
                }
                let mean = v.iter().sum::<f32>() / 4.0;
                let var = v.iter().map(|x| (x - mean) * (x - mean)).sum::<f32>() / 4.0;
                let sd = var.sqrt().max(1e-3);
                for b in 0..4 {
                    t[b][k] = (v[b] - mean) / sd;
                }
            }
        }
        for pos in 0..3 {
            fill(&mut shape_tab[pos], &mut rng);
            fill(&mut sec_tab[pos], &mut rng);
        }
        let mut hyd = [0.0; 64];
        let mut gc = [0.0; 64];
        // Second base: U strongly hydrophobic (as in the real code), A polar.
        let h2 = [1.0, 0.15, -0.9, -0.3];
        let h1 = [0.2, 0.0, -0.1, -0.1];
        for c in 0..64usize {
            let (b1, b2, b3) = split(c as u8);
            hyd[c] = h2[b2] + h1[b1];
            let is_gc = |b: usize| (b == 1 || b == 3) as u8 as f32;
            gc[c] = 0.3 * is_gc(b1) + 0.7 * is_gc(b3);
        }
        Universe { shape_tab, sec_tab, hyd, gc, sites: chem::layout_sites() }
    })
}

#[inline]
pub fn split(c: u8) -> (usize, usize, usize) {
    ((c >> 4) as usize & 3, (c >> 2) as usize & 3, c as usize & 3)
}

pub fn codon_str(c: u8) -> String {
    let (a, b, d) = split(c);
    [BASES[a], BASES[b], BASES[d]].iter().collect()
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Regulator {
    /// Which promoter operators it binds.
    pub op: [f32; 2],
    pub ligand: u8,
    /// How strongly the ligand affects it.
    pub ligand_amp: f32,
    pub activator: bool,
    /// true: active when ligand is present; false: active when absent.
    pub ligand_activates: bool,
    pub strength: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Protein {
    pub len: u16,
    pub shape: [f32; SHAPE_DIM],
    /// 0 = cytoplasmic, 1 = membrane-embedded.
    pub membrane: f32,
    /// 0..1, short chains cannot fold.
    pub fold: f32,
    pub t_opt: f32,
    /// (function index, activity), activities already gated by location/fold.
    pub acts: Vec<(u8, f32)>,
    pub reg: Option<Regulator>,
}

pub fn fold(codons: &[u8]) -> Protein {
    let u = universe();
    let len = codons.len().max(1) as f32;
    let mut s = [0.0f32; SHAPE_DIM];
    let mut sec = [0.0f32; SEC_DIM];
    let (mut hyd, mut gc) = (0.0f32, 0.0f32);
    for &c in codons {
        let (b1, b2, b3) = split(c);
        for k in 0..SHAPE_DIM {
            s[k] += u.shape_tab[0][b1][k] + u.shape_tab[1][b2][k] + 0.25 * u.shape_tab[2][b3][k];
        }
        for k in 0..SEC_DIM {
            sec[k] += u.sec_tab[0][b1][k] + u.sec_tab[1][b2][k] + 0.25 * u.sec_tab[2][b3][k];
        }
        hyd += u.hyd[c as usize & 63];
        gc += u.gc[c as usize & 63];
    }
    let norm = 1.0 / (2.0625 * len).sqrt();
    s.iter_mut().for_each(|v| *v *= norm);
    sec.iter_mut().for_each(|v| *v *= norm);
    hyd /= len;
    gc /= len;
    let membrane = sigmoid((hyd - 0.1) / 0.05);
    let fold = sigmoid((len - 24.0) / 3.0);
    let t_opt = 90.0 * gc;

    let mut acts = Vec::new();
    let inv = 1.0 / (2.0 * SITE_SIGMA * SITE_SIGMA);
    for f in 0..NF {
        let d2: f32 = (0..SHAPE_DIM).map(|k| (s[k] - u.sites[f][k]).powi(2)).sum();
        let mut a = (-d2 * inv).exp() * fold;
        if (chem::F_TRANSPORT0..=chem::F_MOTOR).contains(&f) {
            a *= membrane;
        } else if f == chem::F_REGULATOR {
            a *= 1.0 - membrane;
        }
        if a > 0.003 {
            acts.push((f as u8, a));
        }
    }
    let reg = acts.iter().find(|(f, _)| *f as usize == chem::F_REGULATOR).map(|&(_, a)| {
        let ang = sec[3].atan2(sec[2]).rem_euclid(std::f32::consts::TAU);
        let ligand = ((ang / std::f32::consts::TAU * N_LIGANDS as f32).round() as usize % N_LIGANDS) as u8;
        Regulator {
            op: [sec[0], sec[1]],
            ligand,
            ligand_amp: (sec[2] * sec[2] + sec[3] * sec[3]).sqrt().min(2.0),
            activator: sec[4] > 0.0,
            ligand_activates: sec[5] > 0.0,
            strength: a,
        }
    });
    Protein { len: codons.len() as u16, shape: s, membrane, fold, t_opt, acts, reg }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Gene {
    pub codons: Vec<u8>,
    /// Expression strength of the promoter.
    pub promoter: f32,
    /// Binding site for regulator proteins.
    pub operator: [f32; 2],
    pub protein: Protein,
}

impl Gene {
    pub fn new(codons: Vec<u8>, promoter: f32, operator: [f32; 2]) -> Self {
        let protein = fold(&codons);
        Gene { codons, promoter, operator, protein }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Genome {
    pub genes: Vec<Gene>,
    /// Universal machinery: ribosome promoter strength (share of proteome
    /// devoted to growth machinery).
    pub ribo: f32,
    /// Universal machinery: cell area (µm²) at which division is triggered.
    pub div_size: f32,
    /// Lineage colour hue; drifts a little with every mutation so relatives
    /// look alike and distant lineages diverge.
    pub hue: f32,
    pub total_codons: u32,
    pub has_regulators: bool,
    /// Mutations accumulated since LUCA along this lineage.
    pub mutations: u32,
}

impl Genome {
    pub fn finish(&mut self) {
        self.total_codons = self.genes.iter().map(|g| g.codons.len() as u32).sum();
        self.has_regulators = self.genes.iter().any(|g| g.protein.reg.is_some());
    }

    /// Copy the genome for a daughter cell, with replication errors.
    /// Returns None when the copy is perfect (so the genome can be shared);
    /// otherwise the new genome plus, for each new gene, the index of the
    /// parental gene it descends from.
    pub fn replicate(&self, p: &Params, rng: &mut Rng) -> Option<(Genome, Vec<Option<usize>>)> {
        let total = self.total_codons as f32;
        let ng = self.genes.len() as f32;
        let n_point = rng.poisson(p.mut_point * total);
        let n_indel = rng.poisson(p.mut_indel * total);
        let dup = rng.chance(p.mut_dup);
        let del = rng.chance(p.mut_del);
        let newg = rng.chance(p.mut_new_gene);
        let n_prom = rng.poisson(p.mut_promoter * ng);
        let n_op = rng.poisson(p.mut_operator * ng);
        let m_ribo = rng.chance(p.mut_control);
        let m_div = rng.chance(p.mut_control);
        let events = n_point + n_indel + dup as u32 + del as u32 + newg as u32 + n_prom + n_op + m_ribo as u32 + m_div as u32;
        if events == 0 {
            return None;
        }
        let mut g = self.clone();
        let mut dirty = vec![false; g.genes.len()];
        let mut origin: Vec<Option<usize>> = (0..g.genes.len()).map(Some).collect();
        let max_genes = p.max_genes as usize;

        let pick_codon = |g: &Genome, rng: &mut Rng| -> Option<(usize, usize)> {
            let total: usize = g.genes.iter().map(|x| x.codons.len()).sum();
            if total == 0 {
                return None;
            }
            let mut k = rng.below(total);
            for (gi, gene) in g.genes.iter().enumerate() {
                if k < gene.codons.len() {
                    return Some((gi, k));
                }
                k -= gene.codons.len();
            }
            None
        };

        for _ in 0..n_point {
            if let Some((gi, ci)) = pick_codon(&g, rng) {
                let c = g.genes[gi].codons[ci];
                let pos = rng.below(3);
                let shift = 2 * (2 - pos);
                let old = (c >> shift) & 3;
                let new = (old + 1 + rng.below(3) as u8) & 3;
                g.genes[gi].codons[ci] = (c & !(3 << shift)) | (new << shift);
                dirty[gi] = true;
            }
        }
        for _ in 0..n_indel {
            if let Some((gi, ci)) = pick_codon(&g, rng) {
                let gene = &mut g.genes[gi];
                if rng.chance(0.5) && gene.codons.len() < 400 {
                    gene.codons.insert(ci, rng.below(64) as u8);
                } else if gene.codons.len() > 6 {
                    gene.codons.remove(ci);
                }
                dirty[gi] = true;
            }
        }
        for _ in 0..n_prom {
            if g.genes.is_empty() {
                break;
            }
            let gi = rng.below(g.genes.len());
            let gene = &mut g.genes[gi];
            gene.promoter = (gene.promoter * (rng.normal() * p.mut_promoter_size).exp()).clamp(0.0, 10.0);
        }
        for _ in 0..n_op {
            if g.genes.is_empty() {
                break;
            }
            let gi = rng.below(g.genes.len());
            let gene = &mut g.genes[gi];
            gene.operator[0] += rng.normal() * 0.3;
            gene.operator[1] += rng.normal() * 0.3;
        }
        if m_ribo {
            g.ribo = (g.ribo * (rng.normal() * p.mut_control_size).exp()).clamp(0.05, 20.0);
        }
        if m_div {
            g.div_size = (g.div_size * (rng.normal() * p.mut_control_size).exp()).clamp(6.0, 600.0);
        }
        // Refold changed genes before structural changes shuffle indices.
        for (gi, d) in dirty.iter().enumerate() {
            if *d {
                g.genes[gi].protein = fold(&g.genes[gi].codons);
            }
        }
        if dup && g.genes.len() < max_genes && !g.genes.is_empty() {
            let gi = rng.below(g.genes.len());
            let copy = g.genes[gi].clone();
            g.genes.insert(gi + 1, copy);
            origin.insert(gi + 1, origin[gi]);
        }
        if del && g.genes.len() > 1 {
            let gi = rng.below(g.genes.len());
            g.genes.remove(gi);
            origin.remove(gi);
        }
        if newg && g.genes.len() < max_genes {
            let len = 30 + rng.below(40);
            let codons: Vec<u8> = (0..len).map(|_| rng.below(64) as u8).collect();
            let promoter = rng.range(0.03, 0.3);
            g.genes.push(Gene::new(codons, promoter, [rng.normal(), rng.normal()]));
            origin.push(None);
        }
        g.hue = (g.hue + rng.normal() * 0.012 * (events as f32).sqrt()).rem_euclid(1.0);
        g.mutations += events;
        g.finish();
        Some((g, origin))
    }
}

/// Search for a codon sequence whose protein lands on a target function's
/// site, with the requested location and temperature optimum. Used only to
/// build the ancestral cell at world creation.
pub fn design_gene(target: usize, membrane: bool, t_opt: f32, len: usize, rng: &mut Rng) -> Vec<u8> {
    let u = universe();
    let site = u.sites[target];
    let score = |p: &Protein| -> f32 {
        let d2: f32 = (0..SHAPE_DIM).map(|k| (p.shape[k] - site[k]).powi(2)).sum();
        let loc = if membrane { 1.0 - p.membrane } else { p.membrane };
        d2 + 2.0 * loc * loc + ((p.t_opt - t_opt) / 12.0).powi(2)
    };
    let mut best: Vec<u8> = (0..len).map(|_| rng.below(64) as u8).collect();
    let mut best_s = score(&fold(&best));
    for _ in 0..30000 {
        let mut cand = best.clone();
        let n = 1 + rng.below(2);
        for _ in 0..n {
            let i = rng.below(len);
            cand[i] = rng.below(64) as u8;
        }
        let s = score(&fold(&cand));
        if s < best_s {
            best = cand;
            best_s = s;
            if best_s < 0.004 {
                break;
            }
        }
    }
    best
}

/// The last universal common ancestor: a vent-dwelling chemosynthesizer that
/// makes energy from H₂ + CO₂ (acetogenesis), fixes carbon with H₂, and can
/// import ammonium and phosphate. Everything else must evolve.
pub fn luca(rng: &mut Rng) -> Genome {
    let t = 48.0;
    let mk = |f: usize, mem: bool, len: usize, prom: f32, rng: &mut Rng| {
        let codons = design_gene(f, mem, t, len, rng);
        Gene::new(codons, prom, [rng.normal(), rng.normal()])
    };
    let genes = vec![
        mk(chem::R_ACETOGENESIS, false, 54, 0.40, rng),
        mk(chem::R_H2FIX, false, 50, 0.14, rng),
        mk(chem::F_TRANSPORT0 + 3, true, 46, 0.08, rng), // NH4+ transporter
        mk(chem::F_TRANSPORT0 + 4, true, 46, 0.06, rng), // PO4 transporter
    ];
    let mut g = Genome { genes, ribo: 1.0, div_size: 36.0, hue: 0.07, total_codons: 0, has_regulators: false, mutations: 0 };
    g.finish();
    g
}
