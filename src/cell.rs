//! A single cell: its body, internal chemistry, proteome and life cycle.

use crate::chem::{self, ChemTable, Kind, NF, NR, NS_CELL, NS_GRID, REACTIONS};
use crate::genome::{Genome, LIGAND_ATP};
use crate::params::Params;
use crate::rng::Rng;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const SHAPE_N: usize = 16;
/// Strategy count for ATP bookkeeping (see chem::Strategy).
pub const NSTRAT: usize = 9;
const K_REG: f32 = 40.0;
const MAXG: usize = 256;
const OP_SIGMA: f32 = 0.4;

/// Local environment sampled at the cell's position.
#[derive(Clone, Copy, Default)]
pub struct Env {
    pub temp: f32,
    pub light: f32,
    pub c_out: [f32; NS_GRID],
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Fate {
    Live,
    Divide,
    Die,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Cell {
    pub id: u64,
    pub parent: u64,
    pub lineage: u32,
    pub generation: u32,
    pub born: f64,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub heading: f32,
    pub radius: f32,
    /// Membrane outline: radius multipliers at 16 evenly spaced angles.
    pub shape: [f32; SHAPE_N],
    /// Biomass, equal to cell area in µm².
    pub biomass: f32,
    pub atp: f32,
    /// 0 = pristine, 1 = dead.
    pub damage: f32,
    /// Internal amounts of each species.
    pub amt: [f32; NS_CELL],
    pub genome: Arc<Genome>,
    /// Proteome share of each gene's protein.
    pub expr: Vec<f32>,
    pub expr_ribo: f32,
    /// Current per-function capability (sum of proteome share × activity).
    #[serde(skip, default = "zero_nf")]
    pub cap: [f32; NF],
    /// Smoothed reaction rates (turnovers per µm² per s).
    pub flux: [f32; NR],
    /// Smoothed ATP production rate per strategy (per µm² per s).
    pub atp_src: [f32; NSTRAT],
    pub growth: f32,
    /// DNA replication progress 0..1.
    pub replication: f32,
    /// <0 when not dividing; otherwise pinch progress 0..1.
    pub dividing: f32,
    pub div_axis: f32,
    pub swim: f32,
    pub rng: Rng,
    #[serde(skip)]
    pub gi: u32,
}

fn zero_nf() -> [f32; NF] {
    [0.0; NF]
}

impl Cell {
    pub fn new(id: u64, lineage: u32, x: f32, y: f32, biomass: f32, genome: Arc<Genome>, rng: Rng, born: f64) -> Cell {
        let n = genome.genes.len();
        let mut c = Cell {
            id,
            parent: 0,
            lineage,
            generation: 0,
            born,
            x,
            y,
            vx: 0.0,
            vy: 0.0,
            heading: 0.0,
            radius: (biomass / std::f32::consts::PI).sqrt(),
            shape: [1.0; SHAPE_N],
            biomass,
            atp: biomass * 1.0,
            damage: 0.0,
            amt: [0.0; NS_CELL],
            genome,
            expr: vec![0.0; n],
            expr_ribo: 1.0,
            cap: [0.0; NF],
            flux: [0.0; NR],
            atp_src: [0.0; NSTRAT],
            growth: 0.0,
            replication: 0.0,
            dividing: -1.0,
            div_axis: 0.0,
            swim: 0.0,
            rng,
            gi: 0,
        };
        c.heading = c.rng.range(0.0, std::f32::consts::TAU);
        c.set_expression_to_target();
        let t = c.t_opt();
        c.update_capabilities(t, &Params::default());
        c
    }

    pub fn area(&self) -> f32 {
        self.biomass
    }

    /// Regulation factor per gene from regulator proteins: (1 + activation)
    /// / (1 + repression). Writes into `out`; 1.0 means unregulated.
    fn regulation_into(&self, out: &mut [f32]) {
        let g = &self.genome;
        let n = g.genes.len().min(MAXG);
        let mut act = [0.0f32; MAXG];
        let mut rep = [0.0f32; MAXG];
        if g.has_regulators {
            let area = self.area().max(1e-3);
            for (ti, tf) in g.genes.iter().enumerate().take(n) {
                let Some(reg) = &tf.protein.reg else { continue };
                let level = self.expr.get(ti).copied().unwrap_or(0.0) * reg.strength * K_REG;
                if level < 1e-4 {
                    continue;
                }
                let lig = reg.ligand as usize;
                let c = if lig == LIGAND_ATP { self.atp / area } else { self.amt[lig] / area };
                let theta = c / (c + 0.2);
                let w = (reg.ligand_amp / 1.5).clamp(0.0, 1.0);
                let a = 1.0 - w + w * if reg.ligand_activates { theta } else { 1.0 - theta };
                let eff = level * a;
                for (hi, h) in g.genes.iter().enumerate().take(n) {
                    let d2 = (reg.op[0] - h.operator[0]).powi(2) + (reg.op[1] - h.operator[1]).powi(2);
                    let beta = (-d2 / (2.0 * OP_SIGMA * OP_SIGMA)).exp();
                    if beta < 0.01 {
                        continue;
                    }
                    if reg.activator {
                        act[hi] += eff * beta;
                    } else {
                        rep[hi] += eff * beta;
                    }
                }
            }
        }
        for i in 0..n {
            out[i] = (1.0 + act[i]) / (1.0 + rep[i]);
        }
    }

    /// Target proteome shares from promoters and regulators. Writes one
    /// share per gene into `out` and returns the ribosome share.
    fn expression_targets(&self, out: &mut [f32]) -> f32 {
        let g = &self.genome;
        let n = g.genes.len().min(MAXG);
        self.regulation_into(out);
        let mut z = g.ribo;
        for i in 0..n {
            out[i] *= g.genes[i].promoter;
            z += out[i];
        }
        for v in out.iter_mut().take(n) {
            *v /= z;
        }
        g.ribo / z
    }

    pub fn set_expression_to_target(&mut self) {
        let n = self.genome.genes.len().min(MAXG);
        let mut t = [0.0f32; MAXG];
        let r = self.expression_targets(&mut t);
        self.expr = t[..n].to_vec();
        self.expr_ribo = r;
    }

    /// Regulation factor currently applied to each gene, for the inspector.
    pub fn regulation_factors(&self) -> Vec<f32> {
        let n = self.genome.genes.len().min(MAXG);
        let mut t = [1.0f32; MAXG];
        self.regulation_into(&mut t);
        t[..n].to_vec()
    }

    /// Expression-weighted temperature optimum of the proteome.
    pub fn t_opt(&self) -> f32 {
        let mut s = 0.0;
        let mut w = 0.0;
        for (e, g) in self.expr.iter().zip(&self.genome.genes) {
            s += e * g.protein.t_opt;
            w += e;
        }
        if w > 1e-6 { s / w } else { 40.0 }
    }

    pub fn update_capabilities(&mut self, temp: f32, p: &Params) {
        let mut cap = [0.0f32; NF];
        let w2 = p.thermal_tolerance * p.thermal_tolerance;
        for (e, g) in self.expr.iter().zip(&self.genome.genes) {
            if *e < 1e-6 {
                continue;
            }
            let th = (-(temp - g.protein.t_opt).powi(2) / w2).exp();
            let k = e * th;
            for &(f, a) in &g.protein.acts {
                cap[f as usize] += k * a;
            }
        }
        self.cap = cap;
    }

    /// Phase 1: how much of each species the cell would exchange this step.
    /// Small gases cross the lipid membrane on their own; everything else
    /// needs a transporter protein. Transporters pump actively (spending ATP),
    /// so cells can concentrate scarce nutrients above outside levels.
    pub fn transport_desire(&self, env: &Env, p: &Params, cell_area_grid: f32, dt: f32, flux_in: &mut [f32; NS_GRID]) {
        let area = self.area().max(1e-3);
        let perim = std::f32::consts::TAU * self.radius;
        let ac = self.atp / area;
        let acc = 1.0 + (p.transport_accumulation - 1.0).max(0.0) * ac / (p.km_atp + ac);
        for s in 0..NS_GRID {
            let passive = match s {
                chem::CO2 | chem::H2 | chem::H2S | chem::O2 => p.perm_gas,
                chem::ACETATE => p.perm_acetate,
                _ => 0.0,
            };
            let tr = match chem::TRANSPORTED.iter().position(|&t| t == s) {
                Some(i) => p.transporter_perm * self.cap[chem::F_TRANSPORT0 + i],
                None => 0.0,
            };
            if passive <= 0.0 && tr <= 0.0 {
                flux_in[s] = 0.0;
                continue;
            }
            let c_in = self.amt[s] / area;
            let c_out = env.c_out[s];
            let j_pass = passive * perim * (c_out - c_in);
            let j_tr = tr * perim * (c_out - c_in / acc);
            let mut j = j_pass + j_tr;
            // Never overshoot the equilibrium between cell and its water patch.
            let k = if tr > passive { acc } else { 1.0 };
            let dc = c_out - c_in / k;
            let max_move = dc.abs() * (cell_area_grid * k * area) / (k * area + cell_area_grid) / dt;
            j = j.clamp(-max_move, max_move);
            if j < 0.0 {
                j = j.max(-self.amt[s] / dt * 0.9);
            }
            flux_in[s] = j;
        }
    }

    /// Phase 2: metabolism, growth, damage, replication. `scale` limits uptake
    /// when neighbours compete for the same patch of water.
    #[allow(clippy::too_many_arguments)]
    pub fn metabolize(&mut self, env: &Env, flux_in: &mut [f32; NS_GRID], scale: &[f32; NS_GRID], p: &Params, chem: &ChemTable, dt: f32, can_divide: bool, update_expr: bool) -> Fate {
        let mut pumped = 0.0;
        for s in 0..NS_GRID {
            if flux_in[s] > 0.0 {
                flux_in[s] *= scale[s];
                if chem::TRANSPORTED.contains(&s) {
                    pumped += flux_in[s];
                }
            }
            self.amt[s] = (self.amt[s] + flux_in[s] * dt).max(0.0);
        }
        self.atp = (self.atp - p.transport_atp * pumped * dt).max(0.0);

        let b = self.biomass.max(1e-3);
        let area = b;
        let pace = p.bio_pace;
        let q10f = (p.q10.ln() * (env.temp - 25.0) / 10.0).exp();

        // Gene expression relaxes toward what promoters and regulators dictate
        // (proteins turn over slowly, so this is refreshed every few steps).
        if update_expr {
            let n = self.genome.genes.len().min(MAXG);
            let mut target = [0.0f32; MAXG];
            self.expression_targets(&mut target);
            let k = (p.protein_turnover * pace * dt * 4.0).min(1.0);
            if self.expr.len() == n {
                for (e, t) in self.expr.iter_mut().zip(target.iter()) {
                    *e += (t - *e) * k;
                }
            } else {
                self.expr = target[..n].to_vec();
            }
            self.expr_ribo = (1.0 - self.expr.iter().sum::<f32>()).max(0.0);
            self.update_capabilities(env.temp, p);
        }
        let t_opt = self.t_opt();
        let thermal = (-(env.temp - t_opt).powi(2) / (p.thermal_tolerance * p.thermal_tolerance)).exp();
        let health = (1.0 - self.damage).max(0.0);
        let tf = q10f * pace * health;

        // --- Reactions -------------------------------------------------------
        let light_f = env.light / (env.light + p.light_half_sat);
        let mut atp_gain = [0.0f32; NSTRAT];
        let mut o2_used = 0.0;
        for r in 0..NR {
            let cap = self.cap[r];
            if cap < 1e-5 {
                self.flux[r] *= 0.97;
                continue;
            }
            let rx = &REACTIONS[r];
            let mut sat = 1.0;
            for &(s, _) in rx.subs {
                let c = self.amt[s] / area;
                sat *= c / (chem.km[s] + c);
            }
            if rx.kind == Kind::Light {
                sat *= light_f;
            }
            let atp_d = chem.atp[r];
            if atp_d < 0.0 {
                let ac = self.atp / area;
                sat *= ac / (p.km_atp + ac);
            }
            let mut v = chem.kcat[r] * cap * b * sat * tf;
            for &(s, n) in rx.subs {
                v = v.min(0.5 * self.amt[s] / (n * dt));
            }
            if atp_d < 0.0 {
                v = v.min(0.5 * self.atp / (-atp_d * dt));
            }
            if v <= 0.0 {
                self.flux[r] *= 0.97;
                continue;
            }
            let turn = v * dt;
            for &(s, n) in rx.subs {
                self.amt[s] -= n * turn;
                if s == chem::O2 {
                    o2_used += n * turn;
                }
            }
            for &(s, n) in rx.prods {
                self.amt[s] += n * turn;
            }
            self.atp += atp_d * turn;
            if atp_d > 0.0 {
                atp_gain[rx.strategy as usize] += atp_d * v;
            }
            self.flux[r] += (v / b - self.flux[r]) * 0.03;
        }
        for s in 0..NSTRAT {
            self.atp_src[s] += (atp_gain[s] / b - self.atp_src[s]) * 0.03;
        }

        // --- Reactive oxygen and damage ---------------------------------------
        let c_o2 = self.amt[chem::O2] / area;
        self.amt[chem::ROS] += (p.ros_from_o2 * c_o2 * area * pace * dt + p.ros_leak * o2_used).max(0.0);
        self.amt[chem::ROS] *= (1.0 - p.ros_decay * pace * dt).max(0.0);
        let c_ros = self.amt[chem::ROS] / area;
        self.damage += p.ros_damage * c_ros * pace * dt;
        self.damage += p.aging_rate * q10f * pace * dt;
        let heat_limit = t_opt + 1.5 * p.thermal_tolerance;
        if env.temp > heat_limit {
            self.damage += p.heat_damage * (env.temp - heat_limit) / 10.0 * pace * dt;
        }

        // --- Growth ------------------------------------------------------------
        let sat = |c: f32, km: f32| c / (km + c);
        let gsat = sat(self.amt[chem::SUGAR] / area, chem.km[chem::SUGAR])
            * sat(self.amt[chem::NH4] / area, chem.km[chem::NH4])
            * sat(self.amt[chem::PO4] / area, chem.km[chem::PO4])
            * sat(self.atp / area, p.km_atp);
        let mut g = p.growth_rate * q10f * thermal * pace * health * self.expr_ribo * b * gsat;
        g = g
            .min(0.5 * self.amt[chem::SUGAR] / dt)
            .min(0.5 * self.amt[chem::NH4] / (p.biomass_n.max(1e-6) * dt))
            .min(0.5 * self.amt[chem::PO4] / (p.biomass_p.max(1e-6) * dt))
            .min(0.5 * self.atp / (p.growth_atp.max(1e-6) * dt))
            .max(0.0);
        let gd = g * dt;
        self.amt[chem::SUGAR] -= gd;
        self.amt[chem::NH4] -= p.biomass_n * gd;
        self.amt[chem::PO4] -= p.biomass_p * gd;
        self.atp -= p.growth_atp * gd;
        self.biomass += gd;
        self.growth += (g / b - self.growth) * 0.03;

        // --- Maintenance, starvation, repair ----------------------------------
        let need = p.maintenance * b * q10f * pace * dt;
        if self.atp >= need {
            self.atp -= need;
        } else {
            let deficit = need - self.atp;
            self.atp = 0.0;
            let eaten = if p.autophagy_yield > 0.0 { deficit / p.autophagy_yield } else { 0.0 };
            let eaten = eaten.min(self.biomass);
            self.biomass -= eaten;
            self.amt[chem::CO2] += eaten;
            self.amt[chem::NH4] += eaten * p.biomass_n;
            self.amt[chem::PO4] += eaten * p.biomass_p;
            self.damage += p.starvation_damage * pace * dt;
        }
        if self.damage > 0.0 {
            let rep = self.damage.min(p.repair_rate * q10f * pace * dt);
            let cost = rep * p.repair_atp * b;
            if self.atp >= cost {
                self.atp -= cost;
                self.damage -= rep;
            }
        }
        self.damage = self.damage.max(0.0);
        self.atp = self.atp.min(p.atp_capacity * self.biomass).max(0.0);

        // --- Swimming -----------------------------------------------------------
        let m = self.cap[chem::F_MOTOR];
        if m > 1e-4 {
            let mut speed = p.motor_speed * m / (m + 0.03) / p.viscosity * health;
            let cost = p.motor_cost * b * speed * speed * dt;
            if cost > 0.0 && self.atp < cost {
                speed *= (self.atp / cost).sqrt();
                self.atp = 0.0;
            } else {
                self.atp -= cost;
            }
            self.swim = speed;
            if self.rng.chance(p.tumble_rate * dt) {
                self.heading = self.rng.range(0.0, std::f32::consts::TAU);
            }
        } else {
            self.swim = 0.0;
        }

        // --- DNA replication and division --------------------------------------
        let gcod = self.genome.total_codons.max(1) as f32;
        if self.replication < 1.0 && self.biomass >= 0.5 * self.genome.div_size {
            let mut d = p.replication_speed * q10f * thermal * pace * health * dt / gcod;
            d = d.min(1.0 - self.replication);
            let atp_cost = p.replication_atp * gcod * d;
            let np_cost = p.replication_np * gcod * d;
            let mut f: f32 = 1.0;
            if atp_cost > 0.0 {
                f = f.min(0.5 * self.atp / atp_cost);
            }
            if np_cost > 0.0 {
                f = f.min(0.5 * self.amt[chem::NH4] / np_cost).min(0.5 * self.amt[chem::PO4] / np_cost);
            }
            let f = f.clamp(0.0, 1.0);
            self.replication += d * f;
            self.atp -= atp_cost * f;
            self.amt[chem::NH4] -= np_cost * f;
            self.amt[chem::PO4] -= np_cost * f;
            if self.replication > 0.9999 {
                self.replication = 1.0;
            }
        }
        if self.dividing < 0.0 && self.replication >= 1.0 && self.biomass >= self.genome.div_size && can_divide {
            self.dividing = 0.0;
            self.div_axis = self.rng.range(0.0, std::f32::consts::TAU);
        }
        if self.dividing >= 0.0 {
            self.dividing += dt * pace / p.division_duration;
        }

        for v in self.amt.iter_mut() {
            *v = v.max(0.0);
        }
        self.radius = (self.biomass / std::f32::consts::PI).sqrt();

        if self.damage >= 1.0 || self.biomass < p.min_size * self.genome.div_size {
            return Fate::Die;
        }
        if self.dividing >= 1.0 {
            return Fate::Divide;
        }
        Fate::Live
    }

    /// Fraction of ATP currently coming from each strategy.
    pub fn strategy_mix(&self) -> ([f32; NSTRAT], f32) {
        let total: f32 = self.atp_src.iter().sum();
        let mut mix = [0.0; NSTRAT];
        if total > 1e-6 {
            for i in 0..NSTRAT {
                mix[i] = self.atp_src[i] / total;
            }
        }
        (mix, total)
    }

    pub fn dominant_strategy(&self) -> Option<usize> {
        let (mix, total) = self.strategy_mix();
        if total < 1e-4 {
            return None;
        }
        let (i, v) = mix.iter().enumerate().fold((0, 0.0), |a, (i, &v)| if v > a.1 { (i, v) } else { a });
        if v > 0.0 { Some(i) } else { None }
    }

    /// Colour from how the cell is currently making its energy.
    pub fn color(&self) -> [u8; 3] {
        let (mix, total) = self.strategy_mix();
        let idle = [120.0, 135.0, 160.0];
        if total < 1e-5 {
            return [120, 135, 160];
        }
        let mut c = [0.0f32; 3];
        for i in 0..NSTRAT {
            for k in 0..3 {
                c[k] += mix[i] * chem::STRATEGY_COLORS[i][k] as f32;
            }
        }
        // Fade toward grey when energy production is weak.
        let vigor = (total / 0.02).min(1.0);
        [0, 1, 2].map(|k| (idle[k] + (c[k] - idle[k]) * vigor) as u8)
    }
}
