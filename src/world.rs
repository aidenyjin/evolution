//! The world: a strip of early-Earth seafloor from deep ocean to beach, a
//! grid of dissolved chemicals, slow currents and waves, and the cells.

use crate::cell::{Cell, Env, Fate, SHAPE_N};
use crate::chem::{self, ChemTable, NS_CELL, NS_GRID};
use crate::genome;
use crate::params::Params;
use crate::rng::Rng;
use crate::stats::Stats;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;
use std::sync::Arc;

/// Simulated seconds per step. One step is drawn per frame at 60 fps in
/// real-time mode.
pub const DT: f32 = 1.0 / 60.0;

#[derive(Clone, Serialize, Deserialize)]
pub struct Vent {
    pub x: f32,
    pub y: f32,
    pub phase: [f32; 2],
    pub strength: f32,
    /// Grid cells this vent releases into, with weights summing to 1.
    pub cells: Vec<(u32, f32)>,
    #[serde(default)]
    pub output: f32,
    /// Seconds since the vent opened, and how long it will last (0 = forever).
    #[serde(default)]
    pub age: f32,
    #[serde(default)]
    pub life: f32,
}

impl Vent {
    /// 0..1 activity over the vent's life: it warms up after opening and
    /// fades away before going extinct.
    pub fn envelope(&self) -> f32 {
        if self.life <= 0.0 {
            return 1.0;
        }
        let fade = (self.life * 0.25).clamp(30.0, 300.0);
        smoothstep(0.0, 90.0, self.age) * (1.0 - smoothstep(self.life - fade, self.life, self.age))
    }
}

/// Grid cells a vent at (x, y) releases into, with weights summing to 1.
fn vent_cells(x: f32, y: f32, r: f32, dx: f32, gw: usize, gh: usize) -> Vec<(u32, f32)> {
    let mut cells = Vec::new();
    let (gx0, gx1) = (((x - 3.0 * r) / dx).floor().max(0.0) as usize, (((x + 3.0 * r) / dx).ceil() as usize).min(gw - 1));
    let (gy0, gy1) = (((y - 3.0 * r) / dx).floor().max(0.0) as usize, (((y + 3.0 * r) / dx).ceil() as usize).min(gh - 1));
    let mut total = 0.0;
    for gy in gy0..=gy1 {
        for gx in gx0..=gx1 {
            let cx = (gx as f32 + 0.5) * dx;
            let cy = (gy as f32 + 0.5) * dx;
            let d2 = (cx - x).powi(2) + (cy - y).powi(2);
            let wgt = (-d2 / (2.0 * r * r)).exp();
            if wgt > 0.01 {
                cells.push(((gy * gw + gx) as u32, wgt));
                total += wgt;
            }
        }
    }
    for c in cells.iter_mut() {
        c.1 /= total.max(1e-6);
    }
    cells
}

/// A random spot in the deep zone, away from existing vents.
fn pick_vent_site(rng: &mut Rng, vents: &[Vent], p: &Params, w: f32, h: f32) -> Option<(f32, f32)> {
    let nv = p.vent_count.max(1.0);
    let min_sep = (w * p.deep_end * h / nv).sqrt() * 0.55;
    for _ in 0..2000 {
        let x = rng.range(0.07 * w, (p.deep_end - 0.025).max(0.1) * w);
        let y = rng.range(0.08 * h, 0.92 * h);
        if !vents.iter().any(|v| ((v.x - x).powi(2) + (v.y - y).powi(2)).sqrt() < min_sep) {
            return Some((x, y));
        }
    }
    None
}

#[derive(Clone, Copy)]
pub struct Death {
    pub x: f32,
    pub y: f32,
    pub r: f32,
    pub color: [u8; 3],
}

#[derive(Default)]
pub struct Scratch {
    pos: Vec<[f32; 3]>,
    gis: Vec<u32>,
    xfer: Vec<[f32; NS_GRID]>,
    bin_start: Vec<u32>,
    bin_items: Vec<u32>,
    envs: Vec<Env>,
    demand: Vec<f32>,
    tmp: Vec<Vec<f32>>,
    fates: Vec<Fate>,
    pub deaths: Vec<Death>,
}

#[derive(Serialize, Deserialize)]
pub struct World {
    pub params: Params,
    pub seed: u64,
    pub step: u64,
    pub time: f64,
    pub w: f32,
    pub h: f32,
    pub gw: usize,
    pub gh: usize,
    pub dx: f32,
    /// [species][gy * gw + gx], concentration in units/µm².
    pub grid: Vec<Vec<f32>>,
    /// Zone weights per grid cell: deep, shallow, beach.
    pub zone: Vec<[f32; 3]>,
    /// 0..1 closeness to the nearest vent (heat).
    pub vent_heat: Vec<f32>,
    pub vents: Vec<Vent>,
    /// Phases of the slow climate cycle.
    #[serde(default)]
    pub climate_phase: [f32; 2],
    /// Current ocean temperature offset from the climate cycle (°C).
    #[serde(skip)]
    pub climate_now: f32,
    /// Water velocity per grid cell (µm/s).
    pub flow: Vec<[f32; 2]>,
    flow_modes: Vec<[f32; 5]>,
    #[serde(skip)]
    pub surf: Vec<f32>,
    #[serde(skip)]
    col_sc: Vec<[f32; 2]>,
    #[serde(skip)]
    gex_f: Vec<f32>,
    #[serde(skip)]
    nh4_t: Vec<f32>,
    #[serde(skip)]
    po4_t: Vec<f32>,
    #[serde(skip)]
    adv_counter: u32,
    /// Accumulated seconds per phase (physics, biology, grid, stats).
    #[serde(skip)]
    pub prof: [f64; 4],
    pub cells: Vec<Cell>,
    pub next_id: u64,
    pub rng: Rng,
    pub stats: Stats,
    #[serde(skip)]
    pub chem: Option<ChemTable>,
    #[serde(skip)]
    pub scratch: Scratch,
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Precomputed sample directions of the membrane outline.
fn shape_dirs() -> &'static [(f32, f32); SHAPE_N] {
    use std::sync::OnceLock;
    static D: OnceLock<[(f32, f32); SHAPE_N]> = OnceLock::new();
    D.get_or_init(|| {
        let mut d = [(0.0, 0.0); SHAPE_N];
        for (k, v) in d.iter_mut().enumerate() {
            let a = k as f32 * TAU / SHAPE_N as f32;
            *v = (a.cos(), a.sin());
        }
        d
    })
}

impl World {
    pub fn new(params: Params, seed: u64) -> World {
        let p = params.clone();
        let w = p.world_width;
        let h = p.world_height;
        let dx = p.grid_res;
        let gw = (w / dx).ceil() as usize;
        let gh = (h / dx).ceil() as usize;
        let n = gw * gh;
        let mut rng = Rng::new(seed);

        // Zones: banded from deep (left) to beach (right), with a gently
        // wandering boundary so the coastline looks natural.
        let wig = [rng.range(0.0, TAU), rng.range(0.0, TAU), rng.range(0.0, TAU)];
        let mut zone = vec![[0.0f32; 3]; n];
        for gy in 0..gh {
            for gx in 0..gw {
                let y = (gy as f32 + 0.5) * dx;
                let f = (gx as f32 + 0.5) * dx / w
                    + 0.018 * (y / 170.0 + wig[0]).sin()
                    + 0.010 * (y / 61.0 + wig[1]).sin()
                    + 0.005 * (y / 23.0 + wig[2]).sin();
                let b = p.zone_blend;
                let deep = 1.0 - smoothstep(p.deep_end - b, p.deep_end + b, f);
                let beach = smoothstep(p.shallow_end - b, p.shallow_end + b, f);
                let shallow = (1.0 - deep - beach).max(0.0);
                zone[gy * gw + gx] = [deep, shallow, beach];
            }
        }

        // Vents, scattered through the deep with a minimum spacing. Their
        // lifetimes are staggered so they don't all die at once.
        let mut vents: Vec<Vent> = Vec::new();
        let nv = p.vent_count as usize;
        while vents.len() < nv {
            let Some((x, y)) = pick_vent_site(&mut rng, &vents, &p, w, h) else { break };
            let life = if p.vent_lifetime > 0.0 { p.vent_lifetime * rng.range(0.7, 1.3) } else { 0.0 };
            vents.push(Vent {
                x,
                y,
                phase: [rng.range(0.0, TAU), rng.range(0.0, TAU)],
                strength: rng.range(0.7, 1.3),
                cells: vent_cells(x, y, p.vent_radius, dx, gw, gh),
                output: 1.0,
                age: if life > 0.0 { rng.range(100.0, 0.6 * life) } else { 0.0 },
                life,
            });
        }
        let vent_heat = vec![0.0f32; n];

        let mut flow_modes = Vec::new();
        for _ in 0..5 {
            let ang = rng.range(0.0, TAU);
            let kmag = rng.range(0.7, 1.4);
            flow_modes.push([ang.cos() * kmag, ang.sin() * kmag, rng.range(0.0, TAU), rng.range(0.5, 1.5), rng.range(0.6, 1.0)]);
        }

        let mut world = World {
            params: p,
            seed,
            step: 0,
            time: 0.0,
            w,
            h,
            gw,
            gh,
            dx,
            grid: vec![vec![0.0; n]; NS_GRID],
            zone,
            vent_heat,
            vents,
            climate_phase: [0.0, 0.0],
            climate_now: 0.0,
            flow: vec![[0.0; 2]; n],
            flow_modes,
            surf: Vec::new(),
            col_sc: Vec::new(),
            gex_f: Vec::new(),
            nh4_t: Vec::new(),
            po4_t: Vec::new(),
            adv_counter: 0,
            prof: [0.0; 4],
            cells: Vec::new(),
            next_id: 1,
            rng,
            stats: Stats::new(),
            chem: None,
            scratch: Scratch::default(),
        };
        world.climate_phase = [world.rng.range(0.0, TAU), world.rng.range(0.0, TAU)];
        world.rebuild();
        world.update_vent_heat();
        world.init_chemistry();
        world.spawn_luca();
        world.stats.seed_discoveries(&world.cells);
        world.update_flow();
        world.stats.record(&world.cells, &world.grid, world.dx, world.time, 0.0, true);
        world
    }

    /// Recreate derived state after construction or loading from disk.
    pub fn rebuild(&mut self) {
        self.chem = Some(ChemTable::new(&self.params));
        let n = self.gw * self.gh;
        self.scratch.tmp = vec![vec![0.0; n]; NS_GRID];
        self.scratch.demand = vec![0.0; n * NS_GRID];
        let (dx, gw, gh) = (self.dx, self.gw, self.gh);
        for c in self.cells.iter_mut() {
            c.gi = grid_index(c.x, c.y, dx, gw, gh);
        }
        self.build_wave_tables();
    }

    pub fn params_changed(&mut self) {
        self.chem = Some(ChemTable::new(&self.params));
        self.build_wave_tables();
    }

    fn init_chemistry(&mut self) {
        let p = self.params.clone();
        let n = self.gw * self.gh;
        for i in 0..n {
            let z = self.zone[i];
            self.grid[chem::CO2][i] = p.atm_co2;
            self.grid[chem::O2][i] = p.atm_o2;
            self.grid[chem::NH4][i] = z[0] * p.nh4_deep + z[1] * p.nh4_shallow + z[2] * p.nh4_beach;
            self.grid[chem::PO4][i] = z[0] * p.po4_deep + z[1] * p.po4_shallow + z[2] * p.po4_beach;
            self.grid[chem::SO4][i] = p.so4_background;
        }
        // Let vent plumes develop before life starts (large implicit steps).
        for _ in 0..150 {
            self.grid_step(0.5, true);
        }
    }

    fn spawn_luca(&mut self) {
        let mut grng = self.rng.fork();
        let luca = Arc::new(genome::luca(&mut grng));
        let n = self.params.initial_cells as usize;
        for i in 0..n {
            let (cx, cy) = if self.vents.is_empty() {
                (self.rng.range(0.05, self.params.deep_end) * self.w, self.rng.range(0.1, 0.9) * self.h)
            } else {
                let v = &self.vents[i % self.vents.len()];
                (v.x, v.y)
            };
            let a = self.rng.range(0.0, TAU);
            let d = self.rng.range(8.0, 30.0);
            let x = (cx + a.cos() * d).clamp(5.0, self.w - 5.0);
            let y = (cy + a.sin() * d).clamp(5.0, self.h - 5.0);
            let b = luca.div_size * self.rng.range(0.55, 0.95);
            let id = self.next_id;
            self.next_id += 1;
            // Identical genomes, but each founder lineage gets its own colour.
            let mut g = (*luca).clone();
            g.hue = (i as f32 * 0.618_034).fract();
            let mut c = Cell::new(id, id as u32, x, y, b, Arc::new(g), self.rng.fork(), 0.0);
            c.amt[chem::SUGAR] = 0.4 * b;
            c.amt[chem::NH4] = 0.1 * b;
            c.amt[chem::PO4] = 0.02 * b;
            c.atp = 1.5 * b;
            c.replication = self.rng.range(0.0, 0.9);
            c.gi = grid_index(x, y, self.dx, self.gw, self.gh);
            self.cells.push(c);
        }
    }

    // ---------------------------------------------------------------------
    // Environment queries
    // ---------------------------------------------------------------------

    pub fn daylight(&self) -> f32 {
        if self.params.day_night > 0.5 {
            let s = (self.time as f32 * TAU / self.params.day_length).sin();
            s.max(0.0).powf(0.6)
        } else {
            1.0
        }
    }

    #[inline]
    pub fn light_at(&self, gi: usize, day: f32) -> f32 {
        let z = self.zone[gi];
        let p = &self.params;
        (z[0] * p.light_deep + z[1] * p.light_shallow + z[2] * p.light_beach) * day
    }

    #[inline]
    pub fn temp_at(&self, gi: usize) -> f32 {
        let z = self.zone[gi];
        let p = &self.params;
        let base = z[0] * p.temp_deep + z[1] * p.temp_shallow + z[2] * p.temp_beach + self.climate_now;
        base + self.vent_heat[gi] * (p.vent_temp - base)
    }

    /// Ocean temperature offset from the slow climate cycle.
    pub fn climate_offset(&self, t: f64) -> f32 {
        let p = &self.params;
        if p.climate_amplitude <= 0.0 {
            return 0.0;
        }
        let ph = (t as f32) * TAU / p.climate_period.max(1.0);
        p.climate_amplitude * (0.72 * (ph + self.climate_phase[0]).sin() + 0.28 * (ph * 2.7 + self.climate_phase[1]).sin())
    }

    /// Recompute the vent heat field from each vent's current activity.
    pub fn update_vent_heat(&mut self) {
        let (gw, gh, dx) = (self.gw, self.gh, self.dx);
        let hr = self.params.vent_heat_radius;
        self.vent_heat.iter_mut().for_each(|v| *v = 0.0);
        for v in &self.vents {
            let a = v.strength.min(1.0) * v.envelope();
            if a < 1e-3 {
                continue;
            }
            let reach = 4.0 * hr;
            let (gx0, gx1) = (((v.x - reach) / dx).floor().max(0.0) as usize, (((v.x + reach) / dx).ceil() as usize).min(gw - 1));
            let (gy0, gy1) = (((v.y - reach) / dx).floor().max(0.0) as usize, (((v.y + reach) / dx).ceil() as usize).min(gh - 1));
            for gy in gy0..=gy1 {
                for gx in gx0..=gx1 {
                    let cx = (gx as f32 + 0.5) * dx;
                    let cy = (gy as f32 + 0.5) * dx;
                    let d2 = (cx - v.x).powi(2) + (cy - v.y).powi(2);
                    let hv = a * (-d2 / (2.0 * hr * hr)).exp();
                    let gi = gy * gw + gx;
                    if hv > self.vent_heat[gi] {
                        self.vent_heat[gi] = hv;
                    }
                }
            }
        }
    }

    /// Age vents. As a vent starts to fade, a successor opens nearby along
    /// the same seafloor fissure, so nearby life has somewhere to go; the
    /// old vent disappears when it dies.
    fn update_vents(&mut self, dt: f32) {
        if self.params.vent_lifetime <= 0.0 {
            for v in self.vents.iter_mut() {
                v.life = 0.0;
            }
            return;
        }
        let mut spawn_near = Vec::new();
        for (i, v) in self.vents.iter_mut().enumerate() {
            if v.life <= 0.0 {
                v.life = self.params.vent_lifetime * self.rng.range(0.7, 1.3);
                v.age = v.age.min(v.life * 0.5);
            }
            let fade_start = v.life - (v.life * 0.25).clamp(30.0, 300.0);
            if v.age < fade_start && v.age + dt >= fade_start {
                spawn_near.push(i);
            }
            v.age += dt;
        }
        for i in spawn_near {
            if self.vents.len() >= 24 {
                break;
            }
            let (ox, oy) = (self.vents[i].x, self.vents[i].y);
            let p = self.params.clone();
            let (x0, x1) = (0.05 * self.w, (p.deep_end - 0.02).max(0.08) * self.w);
            let mut site = None;
            for _ in 0..200 {
                let a = self.rng.range(0.0, TAU);
                let d = self.rng.range(50.0, 110.0);
                let (x, y) = (ox + a.cos() * d, oy + a.sin() * d);
                let clear = self.vents.iter().enumerate().all(|(j, v)| j == i || ((v.x - x).powi(2) + (v.y - y).powi(2)).sqrt() > 60.0);
                if x > x0 && x < x1 && y > 0.06 * self.h && y < 0.94 * self.h && clear {
                    site = Some((x, y));
                    break;
                }
            }
            let (x, y) = site.or_else(|| pick_vent_site(&mut self.rng, &self.vents, &p, self.w, self.h)).unwrap_or((ox, oy));
            self.vents.push(Vent {
                x,
                y,
                phase: [self.rng.range(0.0, TAU), self.rng.range(0.0, TAU)],
                strength: self.rng.range(0.7, 1.3),
                cells: vent_cells(x, y, p.vent_radius, self.dx, self.gw, self.gh),
                output: 0.0,
                age: 0.0,
                life: p.vent_lifetime * self.rng.range(0.7, 1.3),
            });
            let text = format!("Vent at ({:.0}, {:.0}) µm is fading; a new one is opening nearby at ({:.0}, {:.0}) µm", ox, oy, x, y);
            self.stats.push_event(self.time, self.step, "environment", text, None, x, y);
        }
        let before = self.vents.len();
        self.vents.retain(|v| v.age < v.life);
        // Keep the count steady if something removed vents without successors.
        let target = self.params.vent_count as usize;
        if self.vents.len() < before {
            while self.vents.len() < target.min(24) {
                let p = self.params.clone();
                let Some((x, y)) = pick_vent_site(&mut self.rng, &self.vents, &p, self.w, self.h) else { break };
                self.vents.push(Vent {
                    x,
                    y,
                    phase: [self.rng.range(0.0, TAU), self.rng.range(0.0, TAU)],
                    strength: self.rng.range(0.7, 1.3),
                    cells: vent_cells(x, y, p.vent_radius, self.dx, self.gw, self.gh),
                    output: 0.0,
                    age: 0.0,
                    life: p.vent_lifetime * self.rng.range(0.7, 1.3),
                });
            }
        }
    }

    fn update_flow(&mut self) {
        let p = &self.params;
        let (gw, gh, dx) = (self.gw, self.gh, self.dx);
        let t = self.time as f32;
        let kbase = TAU / p.current_scale;
        let taper_w = 40.0f32;
        let (w, h) = (self.w, self.h);
        // Stream function on grid nodes; velocity = curl, so the current is
        // divergence-free (water is neither created nor destroyed).
        let mut psi = vec![0.0f32; (gw + 1) * (gh + 1)];
        for jy in 0..=gh {
            for jx in 0..=gw {
                let x = jx as f32 * dx;
                let y = jy as f32 * dx;
                let mut s = 0.0;
                for m in &self.flow_modes {
                    let (kx, ky) = (m[0] * kbase, m[1] * kbase);
                    let kk = (kx * kx + ky * ky).sqrt();
                    let ph = m[2] + t * p.current_drift * TAU * m[3];
                    s += m[4] * (kx * x + ky * y + ph).sin() / kk;
                }
                let gi = (jy.min(gh - 1)) * gw + jx.min(gw - 1);
                let z = self.zone[gi];
                let zf = z[0] * p.current_deep_factor + z[1] + z[2] * 0.7;
                let taper = smoothstep(0.0, taper_w, x) * smoothstep(0.0, taper_w, w - x) * smoothstep(0.0, taper_w, y) * smoothstep(0.0, taper_w, h - y);
                psi[jy * (gw + 1) + jx] = s * zf * taper * p.current_strength * 0.45;
            }
        }
        for gy in 0..gh {
            for gx in 0..gw {
                let i00 = gy * (gw + 1) + gx;
                let p00 = psi[i00];
                let p10 = psi[i00 + 1];
                let p01 = psi[i00 + gw + 1];
                let p11 = psi[i00 + gw + 2];
                let u = ((p01 + p11) - (p00 + p10)) * 0.5 / dx;
                let v = -((p10 + p11) - (p00 + p01)) * 0.5 / dx;
                self.flow[gy * gw + gx] = [u, v];
            }
        }
    }

    /// Precompute the static wave-exposure field and column phase tables.
    fn build_wave_tables(&mut self) {
        let p = &self.params;
        let (gw, gh, dx, w) = (self.gw, self.gh, self.dx, self.w);
        self.surf = vec![0.0; gw * gh];
        for gy in 0..gh {
            for gx in 0..gw {
                let gi = gy * gw + gx;
                let x = (gx as f32 + 0.5) * dx;
                let z = self.zone[gi];
                let surf = z[2] + 0.35 * z[1] * smoothstep(p.shallow_end - 0.15, p.shallow_end, x / w);
                self.surf[gi] = surf * smoothstep(0.0, 25.0, w - x);
            }
        }
        self.col_sc = (0..gw).map(|gx| {
            let a = (gx as f32 + 0.5) * dx / 90.0;
            [a.sin(), a.cos()]
        }).collect();
        self.gex_f = self.zone.iter().map(|z| z[0] * p.gas_exchange_deep + z[1] + z[2] * p.gas_exchange_beach).collect();
        self.nh4_t = self.zone.iter().map(|z| z[0] * p.nh4_deep + z[1] * p.nh4_shallow + z[2] * p.nh4_beach).collect();
        self.po4_t = self.zone.iter().map(|z| z[0] * p.po4_deep + z[1] * p.po4_shallow + z[2] * p.po4_beach).collect();
    }

    /// Wave surge phase terms for the current time: (amp·sin φ, amp·cos φ).
    #[inline]
    pub fn wave_terms(&self) -> (f32, f32) {
        let ph = self.time as f32 * TAU / self.params.wave_period;
        (self.params.wave_strength * ph.sin(), self.params.wave_strength * ph.cos())
    }

    /// Back-and-forth wave velocity (x direction) at a grid cell.
    #[inline]
    fn wave_u(surf: &[f32], col_sc: &[[f32; 2]], gi: usize, gx: usize, wt: (f32, f32)) -> f32 {
        // amp·sin(φ − kx) = amp·(sin φ cos kx − cos φ sin kx)
        surf[gi] * (wt.0 * col_sc[gx][1] - wt.1 * col_sc[gx][0])
    }

    /// Total water velocity (current + waves) at a grid cell.
    pub fn water_at(&self, gi: usize) -> [f32; 2] {
        let wt = self.wave_terms();
        let f = self.flow[gi];
        [f[0] + Self::wave_u(&self.surf, &self.col_sc, gi, gi % self.gw, wt), f[1]]
    }

    #[inline]
    fn sample_flow(flow: &[[f32; 2]], x: f32, y: f32, dx: f32, gw: usize, gh: usize) -> [f32; 2] {
        let fx = (x / dx - 0.5).clamp(0.0, (gw - 1) as f32);
        let fy = (y / dx - 0.5).clamp(0.0, (gh - 1) as f32);
        let x0 = fx.floor() as usize;
        let y0 = fy.floor() as usize;
        let x1 = (x0 + 1).min(gw - 1);
        let y1 = (y0 + 1).min(gh - 1);
        let tx = fx - x0 as f32;
        let ty = fy - y0 as f32;
        let a = flow[y0 * gw + x0];
        let b = flow[y0 * gw + x1];
        let c = flow[y1 * gw + x0];
        let d = flow[y1 * gw + x1];
        let mut r = [0.0; 2];
        for k in 0..2 {
            r[k] = (a[k] * (1.0 - tx) + b[k] * tx) * (1.0 - ty) + (c[k] * (1.0 - tx) + d[k] * tx) * ty;
        }
        r
    }

    // ---------------------------------------------------------------------
    // Main step
    // ---------------------------------------------------------------------

    pub fn step(&mut self) {
        if self.step % 30 == 0 {
            self.update_flow();
            self.update_vent_heat();
        }
        self.climate_now = self.climate_offset(self.time);
        let t0 = std::time::Instant::now();
        self.physics();
        let t1 = std::time::Instant::now();
        self.biology();
        let t2 = std::time::Instant::now();
        // The chemistry grid advances every other step with a double step.
        if self.step % 2 == 1 {
            self.update_vents(2.0 * DT);
            self.grid_step(2.0 * DT, false);
        }
        let t3 = std::time::Instant::now();
        self.prof[0] += (t1 - t0).as_secs_f64();
        self.prof[1] += (t2 - t1).as_secs_f64();
        self.prof[2] += (t3 - t2).as_secs_f64();
        self.step += 1;
        self.time += DT as f64;
        if self.step % 30 == 0 {
            self.stats.record(&self.cells, &self.grid, self.dx, self.time, self.climate_now, false);
            let events = self.stats.check_discoveries(&self.cells, self.time, self.step);
            let _ = events;
        }
    }

    fn physics(&mut self) {
        let p = &self.params;
        let n = self.cells.len();
        if n == 0 {
            return;
        }
        let s = &mut self.scratch;
        self.cells.par_iter().map(|c| [c.x, c.y, c.radius * (1.0 + 0.38 * c.dividing.max(0.0))]).collect_into_vec(&mut s.pos);
        let max_r = s.pos.iter().fold(1.0f32, |m, q| m.max(q[2]));
        let cs = (2.0 * max_r).max(6.0);
        let nx = (self.w / cs).ceil() as usize + 1;
        let ny = (self.h / cs).ceil() as usize + 1;
        let nb = nx * ny;
        s.bin_start.clear();
        s.bin_start.resize(nb + 1, 0);
        let bin_of = |x: f32, y: f32| -> usize {
            let bx = ((x / cs) as usize).min(nx - 1);
            let by = ((y / cs) as usize).min(ny - 1);
            by * nx + bx
        };
        for q in &s.pos {
            s.bin_start[bin_of(q[0], q[1]) + 1] += 1;
        }
        for i in 0..nb {
            s.bin_start[i + 1] += s.bin_start[i];
        }
        s.bin_items.clear();
        s.bin_items.resize(n, 0);
        let mut cursor = s.bin_start.clone();
        for (i, q) in s.pos.iter().enumerate() {
            let b = bin_of(q[0], q[1]);
            s.bin_items[cursor[b] as usize] = i as u32;
            cursor[b] += 1;
        }

        let wt = {
            let ph = self.time as f32 * TAU / p.wave_period;
            (p.wave_strength * ph.sin(), p.wave_strength * ph.cos())
        };
        let surf = &self.surf;
        let col_sc = &self.col_sc;
        let pos = &s.pos;
        let bin_start = &s.bin_start;
        let bin_items = &s.bin_items;
        let flow = &self.flow;
        let (dx, gw, gh, w, h) = (self.dx, self.gw, self.gh, self.w, self.h);
        let dirs = shape_dirs();
        let zone = &self.zone;
        let vent_heat = &self.vent_heat;
        let climate = self.climate_now;
        let temp_of = |gi: usize| {
            let z = zone[gi];
            let base = z[0] * p.temp_deep + z[1] * p.temp_shallow + z[2] * p.temp_beach + climate;
            base + vent_heat[gi] * (p.vent_temp - base)
        };

        s.gis.resize(n, 0);
        let gis = &mut s.gis;
        self.cells.par_iter_mut().zip(gis.par_iter_mut()).enumerate().for_each(|(i, (c, gslot))| {
            let [x, y, r] = pos[i];
            let mut mx = 0.0f32;
            let mut my = 0.0f32;
            let mut squeeze = 0.0f32;
            let mut contacts = 0u32;
            let mut caps = [f32::MAX; SHAPE_N];
            let bx = ((x / cs) as usize).min(nx - 1);
            let by = ((y / cs) as usize).min(ny - 1);
            for yy in by.saturating_sub(1)..=(by + 1).min(ny - 1) {
                for xx in bx.saturating_sub(1)..=(bx + 1).min(nx - 1) {
                    let b = yy * nx + xx;
                    for k in bin_start[b]..bin_start[b + 1] {
                        let j = bin_items[k as usize] as usize;
                        if j == i {
                            continue;
                        }
                        let [xj, yj, rj] = pos[j];
                        let ox = x - xj;
                        let oy = y - yj;
                        let d2 = ox * ox + oy * oy;
                        let rr = r + rj;
                        if d2 >= rr * rr {
                            continue;
                        }
                        let d = d2.sqrt().max(1e-3);
                        let (nxv, nyv) = if d2 > 1e-8 { (ox / d, oy / d) } else { (1.0, 0.0) };
                        let push = (rr - d) * 0.5 * p.collision_stiffness;
                        mx += nxv * push;
                        my += nyv * push;
                        squeeze += (rr - d) / r.max(0.3);
                        contacts += 1;
                        // Membranes flatten against each other at the contact.
                        let pd = d / rr;
                        for (kk, &(cx, cy)) in dirs.iter().enumerate() {
                            let cosd = -(cx * nxv + cy * nyv);
                            if cosd > 0.15 {
                                caps[kk] = caps[kk].min(pd / cosd);
                            }
                        }
                    }
                }
            }
            // Average pushes from many neighbours and cap the step, so packed
            // cells settle instead of jittering.
            if contacts > 1 {
                let k = 1.0 / (0.5 * contacts as f32).max(1.0);
                mx *= k;
                my *= k;
            }
            let mlen = (mx * mx + my * my).sqrt();
            let mcap = 0.3 * r;
            if mlen > mcap {
                mx *= mcap / mlen;
                my *= mcap / mlen;
            }
            c.pressure += (squeeze - c.pressure) * 0.1;
            let mut f = Self::sample_flow(flow, x, y, dx, gw, gh);
            let gi = grid_index(x, y, dx, gw, gh) as usize;
            f[0] += Self::wave_u(surf, col_sc, gi, gi % gw, wt);
            let temp = temp_of(gi);
            let rad = c.radius.max(0.3);
            let d_brown = 0.214 / rad * (temp + 273.15) / 293.15 / p.viscosity * p.brownian;
            let sig = (2.0 * d_brown * DT).sqrt();
            let swim_x = c.swim * c.heading.cos();
            let swim_y = c.swim * c.heading.sin();
            let vx = f[0] * p.flow_coupling + swim_x;
            let vy = f[1] * p.flow_coupling + swim_y;
            let nxp = (x + vx * DT + mx + sig * c.rng.normal()).clamp(c.radius, w - c.radius);
            let nyp = (y + vy * DT + my + sig * c.rng.normal()).clamp(c.radius, h - c.radius);
            let dr = 0.16 / (rad * rad * rad) / p.viscosity * p.brownian.max(0.05);
            c.heading += (2.0 * dr * DT).sqrt() * c.rng.normal();
            c.vx += ((nxp - x) / DT - c.vx) * 0.2;
            c.vy += ((nyp - y) / DT - c.vy) * 0.2;
            c.x = nxp;
            c.y = nyp;

            // Membrane outline: elastic return, tension, contacts, pressure.
            let mut sh = c.shape;
            for v in sh.iter_mut() {
                *v += (1.0 - *v) * p.membrane_relax;
            }
            let mut sm = [0.0f32; SHAPE_N];
            for k in 0..SHAPE_N {
                let a = sh[(k + SHAPE_N - 1) % SHAPE_N];
                let b = sh[(k + 1) % SHAPE_N];
                sm[k] = sh[k] + p.membrane_tension * (0.5 * (a + b) - sh[k]);
            }
            let mut capped = [false; SHAPE_N];
            for k in 0..SHAPE_N {
                if sm[k] > caps[k] {
                    sm[k] += (caps[k] - sm[k]) * 0.6;
                    capped[k] = true;
                }
            }
            let a_c: f32 = (0..SHAPE_N).filter(|&k| capped[k]).map(|k| sm[k] * sm[k]).sum();
            let a_f: f32 = (0..SHAPE_N).filter(|&k| !capped[k]).map(|k| sm[k] * sm[k]).sum();
            if a_f > 1e-3 {
                let target = (SHAPE_N as f32 - a_c).max(a_f * 0.8);
                let sc = (target / a_f).sqrt().clamp(1.0, 1.25);
                for k in 0..SHAPE_N {
                    if !capped[k] {
                        sm[k] *= sc;
                    }
                }
            }
            for v in sm.iter_mut() {
                *v = v.clamp(0.55, 1.45);
            }
            c.shape = sm;
            c.gi = grid_index(c.x, c.y, dx, gw, gh);
            *gslot = c.gi;
        });
    }

    fn biology(&mut self) {
        let n = self.cells.len();
        if n == 0 {
            return;
        }
        let chem = self.chem.clone().unwrap_or_else(|| ChemTable::new(&self.params));
        let day = self.daylight();
        let a_grid = self.dx * self.dx;
        let ngrid = self.gw * self.gh;

        // Local environment of each cell, then how much it wants to exchange.
        let mut envs = std::mem::take(&mut self.scratch.envs);
        envs.resize(n, Env::default());
        let mut xfer = std::mem::take(&mut self.scratch.xfer);
        xfer.resize(n, [0.0; NS_GRID]);
        if self.scratch.gis.len() != n {
            self.scratch.gis = self.cells.iter().map(|c| c.gi).collect();
        }
        {
            let p = &self.params;
            let zone = &self.zone;
            let vent_heat = &self.vent_heat;
            let grid = &self.grid;
            let climate = self.climate_now;
            self.cells.par_iter().zip(envs.par_iter_mut()).zip(xfer.par_iter_mut()).for_each(|((c, e), x)| {
                let gi = c.gi as usize;
                let z = zone[gi];
                let base = z[0] * p.temp_deep + z[1] * p.temp_shallow + z[2] * p.temp_beach + climate;
                e.temp = base + vent_heat[gi] * (p.vent_temp - base);
                e.light = (z[0] * p.light_deep + z[1] * p.light_shallow + z[2] * p.light_beach) * day;
                for s in 0..NS_GRID {
                    e.c_out[s] = grid[s][gi];
                }
                c.transport_desire(e, p, a_grid, DT, x);
            });
        }
        let p = &self.params;

        // Neighbours drawing from the same patch of water share what's there.
        let gis = &self.scratch.gis;
        self.scratch.demand.par_chunks_mut(ngrid).enumerate().for_each(|(s, d)| {
            for (gi, x) in gis.iter().zip(xfer.iter()) {
                if x[s] > 0.0 {
                    d[*gi as usize] += x[s] * DT;
                }
            }
        });
        let demand = &self.scratch.demand;
        let grid = &self.grid;
        let can_divide = (n as f32) < p.max_population;
        let step = self.step;
        let mut fates = std::mem::take(&mut self.scratch.fates);
        fates.clear();
        fates.resize(n, Fate::Live);
        self.cells
            .par_iter_mut()
            .zip(envs.par_iter())
            .zip(xfer.par_iter_mut())
            .zip(fates.par_iter_mut())
            .for_each(|(((c, e), x), fate)| {
                let gi = c.gi as usize;
                let mut scale = [1.0f32; NS_GRID];
                for s in 0..NS_GRID {
                    let d = demand[s * ngrid + gi];
                    if d > 0.0 {
                        scale[s] = (grid[s][gi] * a_grid / d).min(1.0);
                    }
                }
                let update_expr = (step + c.id) % 4 == 0;
                *fate = c.metabolize(e, x, &scale, p, &chem, DT, can_divide, update_expr);
            });

        // Apply exchanges to the water (each species independently).
        {
            let gis = &self.scratch.gis;
            let xfer = &xfer;
            self.grid.par_iter_mut().zip(self.scratch.demand.par_chunks_mut(ngrid)).enumerate().for_each(|(s, (g, d))| {
                for (gi, x) in gis.iter().zip(xfer.iter()) {
                    let gi = *gi as usize;
                    let f = x[s];
                    if f != 0.0 {
                        g[gi] = (g[gi] - f * DT / a_grid).max(0.0);
                    }
                    d[gi] = 0.0;
                }
            });
        }

        // Births and deaths. Walking backwards keeps unprocessed indices
        // stable while swap_remove fills holes from the (processed) tail.
        for i in (0..n).rev() {
            match fates[i] {
                Fate::Live => {}
                Fate::Die(cause) => {
                    let c = self.cells.swap_remove(i);
                    self.lyse(&c);
                    self.stats.deaths += 1;
                    self.stats.deaths_by[cause as usize] += 1;
                }
                Fate::Divide => {
                    let c = self.cells.swap_remove(i);
                    let (a, b) = self.divide(c);
                    self.cells.push(a);
                    self.cells.push(b);
                    self.stats.births += 1;
                }
            }
        }
        self.scratch.envs = envs;
        self.scratch.fates = fates;
        self.scratch.xfer = xfer;
        self.scratch.gis.clear();
    }

    /// A dead cell bursts and its contents return to the water.
    fn lyse(&mut self, c: &Cell) {
        let p = &self.params;
        let a_grid = self.dx * self.dx;
        let gi = c.gi as usize;
        for s in 0..NS_GRID {
            self.grid[s][gi] += c.amt[s] / a_grid;
        }
        self.grid[chem::DOM][gi] += c.biomass / a_grid;
        self.grid[chem::NH4][gi] += c.biomass * p.biomass_n / a_grid;
        self.grid[chem::PO4][gi] += c.biomass * p.biomass_p / a_grid;
        self.scratch.deaths.push(Death { x: c.x, y: c.y, r: c.radius, color: c.color() });
        if self.scratch.deaths.len() > 4000 {
            self.scratch.deaths.drain(0..2000);
        }
    }

    fn divide(&mut self, mut c: Cell) -> (Cell, Cell) {
        let p = &self.params;
        let f = (0.5 + c.rng.normal() * p.division_noise).clamp(0.3, 0.7);
        let r = c.radius;
        let off = 0.95 * r / 2f32.sqrt();
        let (ax, ay) = (c.div_axis.cos(), c.div_axis.sin());
        let make = |frac: f32, sign: f32, c: &mut Cell, next_id: &mut u64| -> Cell {
            let mut rng = c.rng.fork();
            let (genome, expr) = match c.genome.replicate(p, &mut rng) {
                Some((g, origin)) => {
                    let expr = origin.iter().map(|o| o.map(|i| c.expr.get(i).copied().unwrap_or(0.0)).unwrap_or(0.0)).collect();
                    (Arc::new(g), expr)
                }
                None => (c.genome.clone(), c.expr.clone()),
            };
            let id = *next_id;
            *next_id += 1;
            let mut amt = [0.0; NS_CELL];
            for s in 0..NS_CELL {
                amt[s] = c.amt[s] * frac;
            }
            let biomass = c.biomass * frac;
            let mut d = Cell::new(id, c.lineage, c.x + ax * off * sign, c.y + ay * off * sign, biomass, genome, rng, 0.0);
            d.parent = c.id;
            d.generation = c.generation + 1;
            d.amt = amt;
            d.atp = c.atp * frac;
            d.damage = c.damage;
            d.expr = expr;
            d.flux = c.flux;
            d.cap = c.cap;
            d.atp_src = c.atp_src;
            d.growth = c.growth;
            d.thermal = c.thermal;
            d.heading = c.heading + sign * 0.6;
            d.vx = c.vx;
            d.vy = c.vy;
            d.x = d.x.clamp(d.radius, self.w - d.radius);
            d.y = d.y.clamp(d.radius, self.h - d.radius);
            d.gi = grid_index(d.x, d.y, self.dx, self.gw, self.gh);
            d
        };
        let mut next_id = self.next_id;
        let mut a = make(f, 1.0, &mut c, &mut next_id);
        let mut b = make(1.0 - f, -1.0, &mut c, &mut next_id);
        self.next_id = next_id;
        a.born = self.time;
        b.born = self.time;
        (a, b)
    }

    // ---------------------------------------------------------------------
    // Chemistry grid
    // ---------------------------------------------------------------------

    fn grid_step(&mut self, dt: f32, warmup: bool) {
        let p = self.params.clone();
        let (gw, gh) = (self.gw, self.gh);
        let n = gw * gh;
        let a_grid = self.dx * self.dx;
        let t = self.time as f32;

        // Vents.
        for v in self.vents.iter_mut() {
            let ph0 = t * TAU / p.vent_period + v.phase[0];
            let ph1 = t * TAU / (0.37 * p.vent_period) + v.phase[1];
            let out = (v.strength * v.envelope() * (1.0 + p.vent_variability * (0.65 * ph0.sin() + 0.35 * ph1.sin()))).max(0.0);
            v.output = out;
            for &(gi, wgt) in &v.cells {
                let gi = gi as usize;
                let k = out * wgt * dt / a_grid;
                self.grid[chem::H2][gi] += p.vent_h2 * k;
                self.grid[chem::H2S][gi] += p.vent_h2s * k;
                if !warmup {
                    self.grid[chem::NH4][gi] += p.vent_minerals * k;
                    self.grid[chem::PO4][gi] += p.vent_minerals * 0.125 * k;
                }
            }
        }

        if !warmup {
            // Spontaneous chemistry: sulfide oxidation and breakdown of dead matter.
            let mut h2s = std::mem::take(&mut self.grid[chem::H2S]);
            let mut o2 = std::mem::take(&mut self.grid[chem::O2]);
            let mut so4 = std::mem::take(&mut self.grid[chem::SO4]);
            let mut dom = std::mem::take(&mut self.grid[chem::DOM]);
            let mut sugar = std::mem::take(&mut self.grid[chem::SUGAR]);
            let kox = p.sulfide_oxidation * dt;
            let kh = (p.dom_hydrolysis * dt).min(1.0);
            const CH: usize = 4096;
            h2s.par_chunks_mut(CH)
                .zip(o2.par_chunks_mut(CH))
                .zip(so4.par_chunks_mut(CH))
                .zip(dom.par_chunks_mut(CH))
                .zip(sugar.par_chunks_mut(CH))
                .for_each(|((((h2s, o2), so4), dom), sugar)| {
                    for i in 0..h2s.len() {
                        if h2s[i] > 0.0 && o2[i] > 0.0 {
                            let r = (kox * h2s[i] * o2[i]).min(h2s[i]).min(0.5 * o2[i]);
                            h2s[i] -= r;
                            o2[i] -= 2.0 * r;
                            so4[i] += r;
                        }
                        let r = dom[i] * kh;
                        dom[i] -= r;
                        sugar[i] += r;
                    }
                });
            self.grid[chem::H2S] = h2s;
            self.grid[chem::O2] = o2;
            self.grid[chem::SO4] = so4;
            self.grid[chem::DOM] = dom;
            self.grid[chem::SUGAR] = sugar;
        }

        // Per species: diffusion (explicit flux form, conserves mass exactly),
        // then exchange with the atmosphere and sediments.
        let tmp = &mut self.scratch.tmp;
        let gexf = &self.gex_f;
        let nh4_t = &self.nh4_t;
        let po4_t = &self.po4_t;
        self.grid.par_iter_mut().zip(tmp.par_iter_mut()).enumerate().for_each(|(s, (g, t))| {
            if warmup && s != chem::H2 && s != chem::H2S {
                return;
            }
            let d = match chem::SPECIES[s].class {
                chem::SpeciesClass::Gas => p.diff_gas,
                chem::SpeciesClass::Organic => p.diff_organic,
                chem::SpeciesClass::Ion => p.diff_ion,
                _ => p.diff_dom,
            };
            let alpha = d * dt / a_grid;
            let sub = (alpha / 0.25).ceil().max(1.0) as usize;
            let a = alpha / sub as f32;
            for _ in 0..sub {
                diffuse_explicit(g, t, gw, gh, a);
            }
            let gk = p.gas_exchange * dt;
            let mk = (p.mineral_supply * dt).min(1.0);
            match s {
                chem::CO2 | chem::O2 | chem::H2 | chem::H2S => {
                    let target = match s {
                        chem::CO2 => p.atm_co2,
                        chem::O2 => p.atm_o2,
                        _ => 0.0,
                    };
                    let f = if s == chem::H2S { 0.5 } else { 1.0 };
                    for i in 0..n {
                        let k = (gk * f * gexf[i]).min(1.0);
                        g[i] += (target - g[i]) * k;
                    }
                }
                chem::NH4 => (0..n).for_each(|i| g[i] += (nh4_t[i] - g[i]) * mk),
                chem::PO4 => (0..n).for_each(|i| g[i] += (po4_t[i] - g[i]) * mk),
                chem::SO4 => (0..n).for_each(|i| g[i] += (p.so4_background - g[i]) * mk * 0.5),
                chem::SUGAR | chem::ACETATE | chem::DOM => {
                    let k = 1.0 - (p.organic_washout * dt).min(1.0);
                    g.iter_mut().for_each(|v| *v *= k);
                }
                _ => {}
            }
        });

        // Advection by currents and waves (every few grid steps).
        self.adv_counter = self.adv_counter.wrapping_add(1);
        if !warmup && self.adv_counter % 4 == 0 {
            let flow = &self.flow;
            let surf = &self.surf;
            let col_sc = &self.col_sc;
            let wt = {
                let ph = self.time as f32 * TAU / p.wave_period;
                (p.wave_strength * ph.sin(), p.wave_strength * ph.cos())
            };
            let k = dt * 4.0 / self.dx;
            self.grid.par_iter_mut().zip(tmp.par_iter_mut()).for_each(|(g, t)| {
                for gy in 0..gh {
                    for gx in 0..gw {
                        let i = gy * gw + gx;
                        let f = flow[i];
                        let u = f[0] + Self::wave_u(surf, col_sc, i, gx, wt);
                        let sx = (gx as f32 - u * k).clamp(0.0, (gw - 1) as f32);
                        let sy = (gy as f32 - f[1] * k).clamp(0.0, (gh - 1) as f32);
                        let x0 = sx as usize;
                        let y0 = sy as usize;
                        let x1 = (x0 + 1).min(gw - 1);
                        let y1 = (y0 + 1).min(gh - 1);
                        let tx = sx - x0 as f32;
                        let ty = sy - y0 as f32;
                        t[i] = (g[y0 * gw + x0] * (1.0 - tx) + g[y0 * gw + x1] * tx) * (1.0 - ty)
                            + (g[y1 * gw + x0] * (1.0 - tx) + g[y1 * gw + x1] * tx) * ty;
                    }
                }
                std::mem::swap(g, t);
            });
        }
    }

    pub fn take_deaths(&mut self) -> Vec<Death> {
        std::mem::take(&mut self.scratch.deaths)
    }
}

#[inline]
pub fn grid_index(x: f32, y: f32, dx: f32, gw: usize, gh: usize) -> u32 {
    let gx = ((x / dx) as usize).min(gw - 1);
    let gy = ((y / dx) as usize).min(gh - 1);
    (gy * gw + gx) as u32
}

fn diffuse_explicit(g: &mut Vec<f32>, t: &mut Vec<f32>, gw: usize, gh: usize, a: f32) {
    // Reflecting walls: a missing neighbour counts as the cell itself.
    for gy in 0..gh {
        let up = if gy > 0 { gy - 1 } else { gy };
        let dn = if gy + 1 < gh { gy + 1 } else { gy };
        let row = &g[gy * gw..(gy + 1) * gw];
        let ru = &g[up * gw..(up + 1) * gw];
        let rd = &g[dn * gw..(dn + 1) * gw];
        let out = &mut t[gy * gw..(gy + 1) * gw];
        let c0 = row[0];
        out[0] = c0 + a * (row[1.min(gw - 1)] + ru[0] + rd[0] - 3.0 * c0);
        for x in 1..gw - 1 {
            let c = row[x];
            out[x] = c + a * (row[x - 1] + row[x + 1] + ru[x] + rd[x] - 4.0 * c);
        }
        let l = gw - 1;
        let cl = row[l];
        out[l] = cl + a * (row[l - 1] + ru[l] + rd[l] - 3.0 * cl);
    }
    std::mem::swap(g, t);
}
