//! Population statistics, history for graphs, and the discoveries log.
//! Everything here is observation only: it never influences the simulation.

use crate::cell::{Cell, NSTRAT};
use crate::chem::{self, NF, NR, NS_GRID};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Serialize, Deserialize)]
pub struct HistPoint {
    pub t: f64,
    pub pop: u32,
    /// Cells by dominant energy strategy; last slot = no clear source.
    pub strat: [u32; NSTRAT + 1],
    /// Total amount of each grid species in the world.
    pub chem: [f32; NS_GRID],
    pub genes: f32,
    pub max_gen: u32,
    pub lineages: u32,
    pub size: f32,
    pub t_opt: f32,
    pub births: u64,
    pub deaths: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Event {
    pub seq: u64,
    pub t: f64,
    pub step: u64,
    pub kind: String,
    pub text: String,
    pub cell: Option<u64>,
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Stats {
    pub births: u64,
    pub deaths: u64,
    pub history: Vec<HistPoint>,
    pub hist_interval: f64,
    pub last_hist: f64,
    pub events: Vec<Event>,
    pub discovered: Vec<bool>,
    pub strat_discovered: Vec<bool>,
    pub milestone: usize,
    pub peak: usize,
    pub latest: Option<HistPoint>,
    /// Bumped whenever a history point is appended (so clients can sync).
    pub hist_version: u64,
    /// Total events ever logged (sequence numbers are 1..=event_total).
    #[serde(default)]
    pub event_total: u64,
}

const MILESTONES: [usize; 7] = [100, 500, 1000, 2500, 5000, 10000, 25000];
/// A function counts as present when the proteome devotes this much
/// effective enzyme to it (trace side-activities don't count).
pub const CAP_THRESHOLD: f32 = 0.04;
/// ...and as actively used above this many turnovers per µm² per second.
const FLUX_THRESHOLD: f32 = 0.02;

impl Stats {
    pub fn new() -> Self {
        Stats {
            births: 0,
            deaths: 0,
            history: Vec::new(),
            hist_interval: 1.0,
            last_hist: -1e9,
            events: Vec::new(),
            discovered: vec![false; NF],
            strat_discovered: vec![false; NSTRAT],
            milestone: 0,
            peak: 0,
            latest: None,
            hist_version: 0,
            event_total: 0,
        }
    }

    pub fn record(&mut self, cells: &[Cell], grid: &[Vec<f32>], dx: f32, t: f64, force: bool) {
        let mut strat = [0u32; NSTRAT + 1];
        let mut genes = 0.0;
        let mut size = 0.0;
        let mut topt = 0.0;
        let mut max_gen = 0;
        let mut lineages = HashSet::new();
        for c in cells {
            match c.dominant_strategy() {
                Some(s) => strat[s] += 1,
                None => strat[NSTRAT] += 1,
            }
            genes += c.genome.genes.len() as f32;
            size += c.biomass;
            topt += c.t_opt();
            max_gen = max_gen.max(c.generation);
            lineages.insert(c.lineage);
        }
        let n = cells.len().max(1) as f32;
        let a = dx * dx;
        let mut chem = [0.0f32; NS_GRID];
        for s in 0..NS_GRID {
            chem[s] = grid[s].iter().sum::<f32>() * a;
        }
        let pt = HistPoint {
            t,
            pop: cells.len() as u32,
            strat,
            chem,
            genes: genes / n,
            max_gen,
            lineages: lineages.len() as u32,
            size: size / n,
            t_opt: topt / n,
            births: self.births,
            deaths: self.deaths,
        };
        self.peak = self.peak.max(cells.len());
        while self.milestone < MILESTONES.len() && cells.len() >= MILESTONES[self.milestone] {
            self.push_event(t, 0, "milestone", format!("Population passed {}", fmt_int(MILESTONES[self.milestone])), None, 0.0, 0.0);
            self.milestone += 1;
        }
        if cells.is_empty() && self.peak > 0 && !self.events.iter().any(|e| e.kind == "extinction") {
            self.push_event(t, 0, "extinction", "All life has gone extinct".into(), None, 0.0, 0.0);
        }
        if force || t - self.last_hist >= self.hist_interval {
            self.last_hist = t;
            self.history.push(pt.clone());
            self.hist_version += 1;
            if self.history.len() > 3000 {
                // Thin out old history: keep every other point.
                let kept: Vec<HistPoint> = self.history.iter().enumerate().filter(|(i, _)| i % 2 == 0).map(|(_, p)| p.clone()).collect();
                self.history = kept;
                self.hist_interval *= 2.0;
                // Signal clients that history was rewritten, not appended.
                self.hist_version += 1_000_000;
            }
        }
        self.latest = Some(pt);
    }

    fn push_event(&mut self, t: f64, step: u64, kind: &str, text: String, cell: Option<u64>, x: f32, y: f32) {
        self.event_total += 1;
        self.events.push(Event { seq: self.event_total, t, step, kind: kind.to_string(), text, cell, x, y });
        if self.events.len() > 500 {
            self.events.remove(0);
        }
    }

    /// Mark whatever the starting cells can already do, so only genuinely
    /// new abilities are announced later.
    pub fn seed_discoveries(&mut self, cells: &[Cell]) {
        for c in cells {
            for f in 0..NF {
                if c.cap[f] > CAP_THRESHOLD * 0.25 {
                    self.discovered[f] = true;
                }
            }
        }
    }

    /// Log the first time any cell actively uses a new reaction or protein
    /// function, or gets most of its energy from a new strategy.
    pub fn check_discoveries(&mut self, cells: &[Cell], t: f64, step: u64) -> usize {
        let before = self.events.len();
        for c in cells {
            for r in 0..NR {
                if !self.discovered[r] && c.flux[r] > FLUX_THRESHOLD && c.cap[r] > CAP_THRESHOLD {
                    self.discovered[r] = true;
                    if c.generation > 0 {
                        let text = format!(
                            "First cell actively performing {} ({}), generation {}",
                            chem::REACTIONS[r].name,
                            chem::REACTIONS[r].equation,
                            c.generation
                        );
                        self.push_event(t, step, "discovery", text, Some(c.id), c.x, c.y);
                    }
                }
            }
            for f in NR..NF {
                if !self.discovered[f] && c.cap[f] > CAP_THRESHOLD {
                    self.discovered[f] = true;
                    if c.generation > 0 {
                        let text = format!("First cell with a working {}, generation {}", chem::function_name(f).to_lowercase(), c.generation);
                        self.push_event(t, step, "discovery", text, Some(c.id), c.x, c.y);
                    }
                }
            }
            if let Some(s) = c.dominant_strategy() {
                let (mix, _) = c.strategy_mix();
                if !self.strat_discovered[s] && mix[s] > 0.5 {
                    self.strat_discovered[s] = true;
                    if c.generation > 0 {
                        let text = format!("A lineage now lives mainly by {}", chem::STRATEGY_NAMES[s].to_lowercase());
                        self.push_event(t, step, "strategy", text, Some(c.id), c.x, c.y);
                    }
                }
            }
        }
        self.events.len() - before
    }
}

pub fn fmt_int(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}
