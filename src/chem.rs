//! The fixed laws of chemistry: which molecules exist, which reactions are
//! possible, and how much energy each releases. Energies are derived from
//! formation free energies, so every closed cycle of reactions loses energy:
//! evolution cannot discover perpetual motion.
//!
//! Also defines "protein shape space": a 4-D space in which each possible
//! protein function has a site. Proteins whose shape lands near a site carry
//! out that function; related functions sit close together.

use crate::params::Params;
use serde::Serialize;

pub const CO2: usize = 0;
pub const H2: usize = 1;
pub const H2S: usize = 2;
pub const O2: usize = 3;
pub const SUGAR: usize = 4;
pub const ACETATE: usize = 5;
pub const SO4: usize = 6;
pub const NH4: usize = 7;
pub const PO4: usize = 8;
pub const DOM: usize = 9;
/// Reactive oxygen species: exists only inside cells.
pub const ROS: usize = 10;

/// Species tracked on the environment grid.
pub const NS_GRID: usize = 10;
/// Species tracked inside cells (grid species + ROS).
pub const NS_CELL: usize = 11;

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SpeciesClass {
    Gas,
    Organic,
    Ion,
    Polymer,
    Internal,
}

#[derive(Serialize)]
pub struct SpeciesInfo {
    pub name: &'static str,
    pub formula: &'static str,
    pub class: SpeciesClass,
    /// Overlay colour in the UI.
    pub color: [u8; 3],
}

pub const SPECIES: [SpeciesInfo; NS_CELL] = [
    SpeciesInfo { name: "Carbon dioxide", formula: "CO₂", class: SpeciesClass::Gas, color: [170, 170, 190] },
    SpeciesInfo { name: "Hydrogen", formula: "H₂", class: SpeciesClass::Gas, color: [255, 140, 90] },
    SpeciesInfo { name: "Hydrogen sulfide", formula: "H₂S", class: SpeciesClass::Gas, color: [230, 220, 60] },
    SpeciesInfo { name: "Oxygen", formula: "O₂", class: SpeciesClass::Gas, color: [90, 200, 255] },
    SpeciesInfo { name: "Sugar", formula: "CH₂O", class: SpeciesClass::Organic, color: [255, 120, 200] },
    SpeciesInfo { name: "Acetate", formula: "CH₃COO⁻", class: SpeciesClass::Organic, color: [200, 140, 255] },
    SpeciesInfo { name: "Sulfate", formula: "SO₄²⁻", class: SpeciesClass::Ion, color: [255, 230, 150] },
    SpeciesInfo { name: "Ammonium", formula: "NH₄⁺", class: SpeciesClass::Ion, color: [120, 255, 170] },
    SpeciesInfo { name: "Phosphate", formula: "PO₄³⁻", class: SpeciesClass::Ion, color: [255, 180, 80] },
    SpeciesInfo { name: "Dead-cell organics", formula: "DOM", class: SpeciesClass::Polymer, color: [180, 150, 110] },
    SpeciesInfo { name: "Reactive oxygen", formula: "ROS", class: SpeciesClass::Internal, color: [255, 60, 60] },
];

pub const R_ACETOGENESIS: usize = 0;
pub const R_H2FIX: usize = 1;
pub const R_ACET_ASSIM: usize = 2;
pub const R_FERMENT: usize = 3;
pub const R_RESP: usize = 4;
pub const R_ACET_OX: usize = 5;
pub const R_SULF_RED_AC: usize = 6;
pub const R_SULF_RED_H2: usize = 7;
pub const R_SULFIDE_OX: usize = 8;
pub const R_H2_OX: usize = 9;
pub const R_ANOXY_PHOTO: usize = 10;
pub const R_OXY_PHOTO: usize = 11;
pub const R_PROTON_PUMP: usize = 12;
pub const R_ROS_DETOX: usize = 13;
pub const NR: usize = 14;

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Kind {
    /// Energy-releasing: yields ATP.
    Downhill,
    /// Energy-requiring: costs ATP.
    Uphill,
    /// Driven by sunlight.
    Light,
    /// Detoxification: no energy change that matters to the cell.
    Detox,
}

/// Energy-strategy categories used only for display, statistics and colours.
/// They never feed back into the simulation.
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Strategy {
    Chemosynthesis,
    Fermentation,
    Photosynthesis,
    OxygenicPhotosynthesis,
    AerobicRespiration,
    SulfateReduction,
    LightPump,
    Biosynthesis,
    Detox,
}

pub const STRATEGY_NAMES: [&str; 9] = [
    "Chemosynthesis",
    "Fermentation",
    "Anoxygenic photosynthesis",
    "Oxygenic photosynthesis",
    "Aerobic respiration",
    "Sulfate reduction",
    "Light-driven pump",
    "Biosynthesis",
    "Detoxification",
];

pub const STRATEGY_COLORS: [[u8; 3]; 9] = [
    [255, 150, 70],  // chemosynthesis: vent orange
    [190, 120, 220], // fermentation: violet
    [200, 120, 90],  // anoxygenic photo: purple-brown (like purple sulfur bacteria)
    [90, 230, 110],  // oxygenic photo: green
    [255, 90, 120],  // aerobic respiration: rose
    [235, 215, 80],  // sulfate reduction: sulfur yellow
    [250, 110, 180], // light pump: magenta (bacteriorhodopsin)
    [160, 170, 190],
    [160, 170, 190],
];

pub struct Reaction {
    pub name: &'static str,
    pub equation: &'static str,
    pub subs: &'static [(usize, f32)],
    pub prods: &'static [(usize, f32)],
    /// Implicit water and protons on the product side (negative = consumed).
    pub h2o: f32,
    pub hplus: f32,
    pub kind: Kind,
    pub strategy: Strategy,
}

pub const REACTIONS: [Reaction; NR] = [
    Reaction { name: "Acetogenesis", equation: "4 H₂ + 2 CO₂ → acetate",
        subs: &[(H2, 4.0), (CO2, 2.0)], prods: &[(ACETATE, 1.0)], h2o: 2.0, hplus: 1.0,
        kind: Kind::Downhill, strategy: Strategy::Chemosynthesis },
    Reaction { name: "H₂ carbon fixation", equation: "2 H₂ + CO₂ → sugar",
        subs: &[(H2, 2.0), (CO2, 1.0)], prods: &[(SUGAR, 1.0)], h2o: 1.0, hplus: 0.0,
        kind: Kind::Uphill, strategy: Strategy::Biosynthesis },
    Reaction { name: "Acetate assimilation", equation: "acetate → 2 sugar",
        subs: &[(ACETATE, 1.0)], prods: &[(SUGAR, 2.0)], h2o: 0.0, hplus: -1.0,
        kind: Kind::Uphill, strategy: Strategy::Biosynthesis },
    Reaction { name: "Fermentation", equation: "3 sugar → acetate + CO₂ + 2 H₂",
        subs: &[(SUGAR, 3.0)], prods: &[(ACETATE, 1.0), (CO2, 1.0), (H2, 2.0)], h2o: -1.0, hplus: 1.0,
        kind: Kind::Downhill, strategy: Strategy::Fermentation },
    Reaction { name: "Aerobic respiration", equation: "sugar + O₂ → CO₂",
        subs: &[(SUGAR, 1.0), (O2, 1.0)], prods: &[(CO2, 1.0)], h2o: 1.0, hplus: 0.0,
        kind: Kind::Downhill, strategy: Strategy::AerobicRespiration },
    Reaction { name: "Acetate oxidation", equation: "acetate + 2 O₂ → 2 CO₂",
        subs: &[(ACETATE, 1.0), (O2, 2.0)], prods: &[(CO2, 2.0)], h2o: 2.0, hplus: -1.0,
        kind: Kind::Downhill, strategy: Strategy::AerobicRespiration },
    Reaction { name: "Sulfate reduction (acetate)", equation: "acetate + SO₄²⁻ → H₂S + 2 CO₂",
        subs: &[(ACETATE, 1.0), (SO4, 1.0)], prods: &[(H2S, 1.0), (CO2, 2.0)], h2o: 2.0, hplus: -3.0,
        kind: Kind::Downhill, strategy: Strategy::SulfateReduction },
    Reaction { name: "Sulfate reduction (H₂)", equation: "4 H₂ + SO₄²⁻ → H₂S",
        subs: &[(H2, 4.0), (SO4, 1.0)], prods: &[(H2S, 1.0)], h2o: 4.0, hplus: -2.0,
        kind: Kind::Downhill, strategy: Strategy::SulfateReduction },
    Reaction { name: "Sulfide oxidation", equation: "H₂S + 2 O₂ → SO₄²⁻",
        subs: &[(H2S, 1.0), (O2, 2.0)], prods: &[(SO4, 1.0)], h2o: 0.0, hplus: 2.0,
        kind: Kind::Downhill, strategy: Strategy::AerobicRespiration },
    Reaction { name: "Hydrogen oxidation", equation: "2 H₂ + O₂ → 2 H₂O",
        subs: &[(H2, 2.0), (O2, 1.0)], prods: &[], h2o: 2.0, hplus: 0.0,
        kind: Kind::Downhill, strategy: Strategy::AerobicRespiration },
    Reaction { name: "Anoxygenic photosynthesis", equation: "H₂S + 2 CO₂ + light → 2 sugar + SO₄²⁻",
        subs: &[(H2S, 1.0), (CO2, 2.0)], prods: &[(SUGAR, 2.0), (SO4, 1.0)], h2o: -2.0, hplus: 2.0,
        kind: Kind::Light, strategy: Strategy::Photosynthesis },
    Reaction { name: "Oxygenic photosynthesis", equation: "CO₂ + H₂O + light → sugar + O₂",
        subs: &[(CO2, 1.0)], prods: &[(SUGAR, 1.0), (O2, 1.0)], h2o: -1.0, hplus: 0.0,
        kind: Kind::Light, strategy: Strategy::OxygenicPhotosynthesis },
    Reaction { name: "Light-driven proton pump", equation: "light → ATP",
        subs: &[], prods: &[], h2o: 0.0, hplus: 0.0,
        kind: Kind::Light, strategy: Strategy::LightPump },
    Reaction { name: "ROS detoxification", equation: "ROS → harmless",
        subs: &[(ROS, 1.0)], prods: &[], h2o: 0.0, hplus: 0.0,
        kind: Kind::Detox, strategy: Strategy::Detox },
];

const G_H2O: f32 = -237.2;
/// Proton at pH 7.
const G_HPLUS: f32 = -39.9;

/// Reaction energetics derived from the current parameters.
#[derive(Clone)]
pub struct ChemTable {
    /// Free energy change per turnover (kJ/mol). NaN for light/detox.
    pub dg: [f32; NR],
    /// ATP gained (positive) or spent (negative) per turnover.
    pub atp: [f32; NR],
    pub kcat: [f32; NR],
    pub km: [f32; NS_CELL],
}

impl ChemTable {
    pub fn new(p: &Params) -> Self {
        let g = |s: usize| -> f32 {
            match s {
                CO2 => p.g_co2,
                H2 => p.g_h2,
                H2S => p.g_h2s,
                O2 => p.g_o2,
                SUGAR => p.g_sugar,
                ACETATE => p.g_acetate,
                SO4 => p.g_so4,
                _ => 0.0,
            }
        };
        let mut dg = [f32::NAN; NR];
        let mut atp = [0.0; NR];
        for (i, r) in REACTIONS.iter().enumerate() {
            match r.kind {
                Kind::Downhill | Kind::Uphill => {
                    let prod: f32 = r.prods.iter().map(|&(s, n)| n * g(s)).sum();
                    let sub: f32 = r.subs.iter().map(|&(s, n)| n * g(s)).sum();
                    let d = prod - sub + r.h2o * G_H2O + r.hplus * G_HPLUS;
                    dg[i] = d;
                    atp[i] = if d < 0.0 {
                        -d * p.efficiency / p.atp_energy
                    } else {
                        let cost = d / (p.efficiency * p.atp_energy);
                        let carbons: f32 = r.prods.iter().filter(|(s, _)| *s == SUGAR).map(|(_, n)| n).sum();
                        -cost.max(p.min_fix_cost * carbons.max(1.0))
                    };
                }
                Kind::Light => {
                    atp[i] = if i == R_PROTON_PUMP { p.pump_atp } else { p.photo_atp };
                }
                Kind::Detox => {}
            }
        }
        let kcat = [
            p.k_acetogenesis, p.k_h2fix, p.k_acet_assim, p.k_ferment, p.k_resp, p.k_acet_ox,
            p.k_sulf_red_ac, p.k_sulf_red_h2, p.k_sulfide_ox, p.k_h2_ox, p.k_anoxy_photo,
            p.k_oxy_photo, p.k_proton_pump, p.k_ros_detox,
        ];
        let mut km = [0.0; NS_CELL];
        for (s, info) in SPECIES.iter().enumerate() {
            km[s] = match info.class {
                SpeciesClass::Gas => p.km_gas,
                SpeciesClass::Organic | SpeciesClass::Polymer => p.km_organic,
                SpeciesClass::Ion => p.km_ion,
                SpeciesClass::Internal => 0.05,
            };
        }
        ChemTable { dg, atp, kcat, km }
    }
}

// ---------------------------------------------------------------------------
// Protein function space
// ---------------------------------------------------------------------------

/// Species that need a transporter protein to cross the membrane.
pub const TRANSPORTED: [usize; 5] = [SUGAR, ACETATE, SO4, NH4, PO4];
pub const F_TRANSPORT0: usize = NR;
pub const F_MOTOR: usize = NR + 5;
pub const F_REGULATOR: usize = NR + 6;
pub const NF: usize = NR + 7;

pub fn function_name(f: usize) -> String {
    if f < NR {
        REACTIONS[f].name.to_string()
    } else if f < F_MOTOR {
        format!("{} transporter", SPECIES[TRANSPORTED[f - F_TRANSPORT0]].formula)
    } else if f == F_MOTOR {
        "Motor (thrust)".to_string()
    } else {
        "Gene regulator".to_string()
    }
}

pub const SHAPE_DIM: usize = 4;
/// Width of each function's site: how precisely a protein must be shaped.
pub const SITE_SIGMA: f32 = 0.42;

/// Pairs of functions that are evolutionary neighbours (sites placed close).
const RELATED: &[(usize, usize)] = &[
    (R_ACETOGENESIS, R_H2FIX),
    (R_ACETOGENESIS, R_SULF_RED_H2),
    (R_ACETOGENESIS, R_ACET_ASSIM),
    (R_ACET_ASSIM, R_FERMENT),
    (R_FERMENT, R_RESP),
    (R_RESP, R_ACET_OX),
    (R_ACET_OX, R_SULF_RED_AC),
    (R_SULF_RED_AC, R_SULF_RED_H2),
    (R_SULFIDE_OX, R_H2_OX),
    (R_H2_OX, R_RESP),
    (R_SULFIDE_OX, R_ANOXY_PHOTO),
    (R_ANOXY_PHOTO, R_OXY_PHOTO),
    (R_ANOXY_PHOTO, R_PROTON_PUMP),
    (R_H2FIX, R_ANOXY_PHOTO),
    (R_ROS_DETOX, R_H2_OX),
    (F_TRANSPORT0, F_TRANSPORT0 + 1),
    (F_TRANSPORT0 + 1, F_TRANSPORT0 + 2),
    (F_TRANSPORT0 + 2, F_TRANSPORT0 + 3),
    (F_TRANSPORT0 + 3, F_TRANSPORT0 + 4),
    (F_TRANSPORT0 + 2, F_TRANSPORT0 + 4),
    (F_MOTOR, F_TRANSPORT0 + 3),
];

/// Lay out function sites in shape space with a deterministic stress
/// minimisation: related functions ~0.8 apart, unrelated ones at least 1.35
/// apart, all within the region random proteins typically occupy.
pub fn layout_sites() -> [[f32; SHAPE_DIM]; NF] {
    let mut rng = crate::rng::Rng::new(0x5EED_0F_11FE);
    let mut pos = [[0.0f32; SHAPE_DIM]; NF];
    for p in pos.iter_mut() {
        for v in p.iter_mut() {
            *v = rng.normal() * 0.9;
        }
    }
    let related = |a: usize, b: usize| RELATED.iter().any(|&(x, y)| (x == a && y == b) || (x == b && y == a));
    let (d_rel, d_min, r_max) = (0.8f32, 1.35f32, 1.55f32);
    for iter in 0..4000 {
        let lr = 0.05 * (1.0 - iter as f32 / 4000.0) + 0.002;
        let mut grad = [[0.0f32; SHAPE_DIM]; NF];
        for a in 0..NF {
            for b in (a + 1)..NF {
                let mut d = [0.0; SHAPE_DIM];
                let mut dist2 = 0.0;
                for k in 0..SHAPE_DIM {
                    d[k] = pos[a][k] - pos[b][k];
                    dist2 += d[k] * d[k];
                }
                let dist = dist2.sqrt().max(1e-4);
                let err = if related(a, b) {
                    dist - d_rel
                } else if dist < d_min {
                    dist - d_min
                } else {
                    0.0
                };
                for k in 0..SHAPE_DIM {
                    let gk = err * d[k] / dist;
                    grad[a][k] += gk;
                    grad[b][k] -= gk;
                }
            }
            let r: f32 = pos[a].iter().map(|v| v * v).sum::<f32>().sqrt();
            if r > r_max {
                for k in 0..SHAPE_DIM {
                    grad[a][k] += (r - r_max) * pos[a][k] / r;
                }
            }
        }
        for a in 0..NF {
            for k in 0..SHAPE_DIM {
                pos[a][k] -= lr * grad[a][k];
            }
        }
    }
    pos
}
