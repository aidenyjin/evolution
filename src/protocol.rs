//! Encoding of what the browser sees: compact binary frames for cells and
//! chemistry, JSON for world info, statistics and the cell inspector.

use crate::cell::Cell;
use crate::chem::{self, NF, NR, NS_CELL, NS_GRID};
use crate::genome::{codon_str, N_LIGANDS};
use crate::params::Params;
use crate::world::{Death, World};
use serde_json::{json, Value};

pub const MSG_CELLS: u8 = 1;
pub const MSG_GRID: u8 = 2;
pub const MSG_STATIC: u8 = 3;
pub const CELL_BYTES: usize = 52;

fn put_f32(b: &mut Vec<u8>, v: f32) {
    b.extend_from_slice(&v.to_le_bytes());
}
fn put_u32(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_le_bytes());
}
fn u8n(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// Cell frame: header + 52 bytes per cell + 16 bytes per recent death.
pub fn encode_cells(w: &World, deaths: &[Death]) -> Vec<u8> {
    let p = &w.params;
    let mut b = Vec::with_capacity(32 + w.cells.len() * CELL_BYTES + deaths.len() * 16);
    b.push(MSG_CELLS);
    b.push(1);
    b.extend_from_slice(&[0, 0]);
    put_u32(&mut b, w.cells.len() as u32);
    b.extend_from_slice(&w.time.to_le_bytes());
    b.extend_from_slice(&w.step.to_le_bytes());
    put_u32(&mut b, deaths.len() as u32);
    put_f32(&mut b, w.daylight());
    for c in &w.cells {
        put_u32(&mut b, c.id as u32);
        put_f32(&mut b, c.x);
        put_f32(&mut b, c.y);
        put_f32(&mut b, c.radius);
        put_f32(&mut b, c.heading);
        for s in c.shape {
            b.push((s * 100.0).clamp(0.0, 255.0) as u8);
        }
        b.push(if c.dividing >= 0.0 { 1 + (c.dividing.clamp(0.0, 1.0) * 254.0) as u8 } else { 0 });
        b.push((c.div_axis.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU * 255.0) as u8);
        b.push(u8n(c.atp / (p.atp_capacity * c.biomass).max(1e-6)));
        b.push(u8n(c.damage));
        b.push(u8n(c.swim / p.motor_speed.max(1e-3)));
        let photo = c.cap[chem::R_ANOXY_PHOTO] + c.cap[chem::R_OXY_PHOTO] + c.cap[chem::R_PROTON_PUMP];
        b.push(u8n(photo * 4.0));
        b.push(c.genome.genes.len().min(255) as u8);
        let mem: f32 = c.expr.iter().zip(&c.genome.genes).map(|(e, g)| e * g.protein.membrane).sum();
        b.push(u8n(mem * 2.5));
        let col = c.color();
        b.extend_from_slice(&col);
        b.push((c.genome.hue * 255.0) as u8);
        b.push(u8n(c.replication));
        let area = c.biomass.max(1e-3);
        b.push(u8n((c.amt[chem::SUGAR] + c.amt[chem::ACETATE]) / area / 3.0));
        b.push(u8n(c.amt[chem::ROS] / area * 5.0));
        b.push(c.generation.min(255) as u8);
    }
    for d in deaths {
        put_f32(&mut b, d.x);
        put_f32(&mut b, d.y);
        put_f32(&mut b, d.r);
        b.extend_from_slice(&d.color);
        b.push(0);
    }
    b
}

/// Chemistry frame: per-species u8 planes (sqrt-encoded relative to a
/// per-species scale) plus the water velocity as two i8 planes.
pub fn encode_grid(w: &World) -> Vec<u8> {
    let n = w.gw * w.gh;
    let mut b = Vec::with_capacity(16 + NS_GRID * 4 + n * (NS_GRID + 2));
    b.push(MSG_GRID);
    b.push(NS_GRID as u8);
    b.extend_from_slice(&(w.gw as u16).to_le_bytes());
    b.extend_from_slice(&(w.gh as u16).to_le_bytes());
    b.extend_from_slice(&[0, 0]);
    put_f32(&mut b, w.daylight());
    let mut scales = [0.0f32; NS_GRID];
    for s in 0..NS_GRID {
        let mx = w.grid[s].iter().fold(0.0f32, |m, &v| m.max(v));
        scales[s] = mx.max(1e-6);
        put_f32(&mut b, scales[s]);
    }
    let wt = w.wave_terms();
    let _ = wt;
    let mut vel = Vec::with_capacity(n);
    let mut vmax = 1e-3f32;
    for gi in 0..n {
        let v = w.water_at(gi);
        vmax = vmax.max(v[0].abs()).max(v[1].abs());
        vel.push(v);
    }
    put_f32(&mut b, vmax);
    for s in 0..NS_GRID {
        let inv = 1.0 / scales[s];
        b.extend(w.grid[s].iter().map(|&v| ((v * inv).max(0.0).sqrt() * 255.0) as u8));
    }
    for k in 0..2 {
        b.extend(vel.iter().map(|v| ((v[k] / vmax).clamp(-1.0, 1.0) * 127.0).round() as i8 as u8));
    }
    b
}

/// Static fields: zone weights and vent heat as an RGBA texture.
pub fn encode_static(w: &World) -> Vec<u8> {
    let n = w.gw * w.gh;
    let mut b = Vec::with_capacity(8 + n * 4);
    b.push(MSG_STATIC);
    b.push(0);
    b.extend_from_slice(&(w.gw as u16).to_le_bytes());
    b.extend_from_slice(&(w.gh as u16).to_le_bytes());
    b.extend_from_slice(&[0, 0]);
    for gi in 0..n {
        let z = w.zone[gi];
        b.push(u8n(z[0]));
        b.push(u8n(z[1]));
        b.push(u8n(z[2]));
        b.push(u8n(w.vent_heat[gi]));
    }
    b
}

pub fn info_json(w: &World, paused: bool, speed: &Value) -> Value {
    let chem = crate::chem::ChemTable::new(&w.params);
    let reactions: Vec<Value> = (0..NR)
        .map(|r| {
            let rx = &chem::REACTIONS[r];
            json!({
                "name": rx.name, "equation": rx.equation, "atp": chem.atp[r],
                "dg": if chem.dg[r].is_finite() { json!(chem.dg[r]) } else { Value::Null },
                "kind": rx.kind, "strategy": rx.strategy as usize,
            })
        })
        .collect();
    let strategies: Vec<Value> =
        (0..chem::STRATEGY_NAMES.len()).map(|i| json!({"name": chem::STRATEGY_NAMES[i], "color": chem::STRATEGY_COLORS[i]})).collect();
    let functions: Vec<String> = (0..NF).map(chem::function_name).collect();
    let vents: Vec<Value> = w.vents.iter().map(|v| json!({"x": v.x, "y": v.y, "strength": v.strength})).collect();
    let values: serde_json::Map<String, Value> = w.params.values().into_iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
    json!({
        "type": "info",
        "world": {
            "w": w.w, "h": w.h, "gw": w.gw, "gh": w.gh, "dx": w.dx,
            "deep_end": w.params.deep_end, "shallow_end": w.params.shallow_end,
        },
        "seed": w.seed,
        "time": w.time,
        "step": w.step,
        "vents": vents,
        "species": chem::SPECIES,
        "reactions": reactions,
        "strategies": strategies,
        "functions": functions,
        "death_causes": crate::cell::CAUSE_NAMES,
        "sites": crate::genome::universe().sites.to_vec(),
        "params": { "desc": Params::descriptors(), "values": values },
        "history": w.stats.history,
        "hist_version": w.stats.hist_version,
        "events": w.stats.events,
        "paused": paused,
        "speed": speed,
        "cell_bytes": crate::protocol::CELL_BYTES,
    })
}

pub fn stats_json(w: &World, sps: f32, paused: bool, speed: &Value) -> Value {
    let vents: Vec<Value> = w.vents.iter().map(|v| json!({"x": v.x, "y": v.y, "output": v.output, "env": v.envelope()})).collect();
    let last_events: Vec<&crate::stats::Event> = w.stats.events.iter().rev().take(20).collect();
    json!({
        "type": "stats",
        "time": w.time,
        "step": w.step,
        "sps": sps,
        "paused": paused,
        "speed": speed,
        "latest": w.stats.latest,
        "hist_version": w.stats.hist_version,
        "last_point": w.stats.history.last(),
        "event_total": w.stats.event_total,
        "events": last_events,
        "vents": vents,
        "daylight": w.daylight(),
        "climate": w.climate_now,
        "births": w.stats.births,
        "deaths": w.stats.deaths,
        "peak": w.stats.peak,
    })
}

fn protein_json(c: &Cell, gi: usize, reg_f: f32) -> Value {
    let g = &c.genome.genes[gi];
    let pr = &g.protein;
    let mut acts: Vec<(u8, f32)> = pr.acts.clone();
    acts.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let acts: Vec<Value> = acts.iter().map(|(f, a)| json!({"f": f, "name": chem::function_name(*f as usize), "a": a})).collect();
    let reg = pr.reg.as_ref().map(|r| {
        let lig = r.ligand as usize;
        let lname = if lig < NS_CELL { chem::SPECIES[lig].formula.to_string() } else { "ATP".to_string() };
        let _ = N_LIGANDS;
        // Which genes in this genome this regulator binds.
        let targets: Vec<Value> = c
            .genome
            .genes
            .iter()
            .enumerate()
            .filter_map(|(hi, h)| {
                let d2 = (r.op[0] - h.operator[0]).powi(2) + (r.op[1] - h.operator[1]).powi(2);
                let beta = (-d2 / (2.0 * 0.4 * 0.4)).exp();
                (beta > 0.05).then(|| json!({"gene": hi, "beta": beta}))
            })
            .collect();
        json!({
            "ligand": lname, "activator": r.activator, "ligand_activates": r.ligand_activates,
            "strength": r.strength, "ligand_amp": r.ligand_amp, "targets": targets,
        })
    });
    let seq: Vec<String> = g.codons.iter().map(|&c| codon_str(c)).collect();
    json!({
        "index": gi,
        "promoter": g.promoter,
        "operator": g.operator,
        "expr": c.expr.get(gi).copied().unwrap_or(0.0),
        "reg_factor": reg_f,
        "len": pr.len,
        "membrane": pr.membrane,
        "fold": pr.fold,
        "t_opt": pr.t_opt,
        "shape": pr.shape,
        "acts": acts,
        "reg": reg,
        "seq": seq.join(" "),
    })
}

pub fn inspect_json(w: &World, id: u64) -> Value {
    let Some(c) = w.cells.iter().find(|c| c.id as u32 == id as u32) else {
        return json!({"type": "inspect", "id": id, "found": false});
    };
    let area = c.biomass.max(1e-3);
    let gi = c.gi as usize;
    let internal: Vec<f32> = (0..NS_CELL).map(|s| c.amt[s] / area).collect();
    let external: Vec<f32> = (0..NS_GRID).map(|s| w.grid[s][gi]).collect();
    let reg = c.regulation_factors();
    let genes: Vec<Value> = (0..c.genome.genes.len()).map(|i| protein_json(c, i, reg.get(i).copied().unwrap_or(1.0))).collect();
    let (mix, atp_rate) = c.strategy_mix();
    let water = w.water_at(gi);
    json!({
        "type": "inspect",
        "id": c.id as u32,
        "found": true,
        "cell": {
            "id": c.id, "parent": c.parent, "lineage": c.lineage, "generation": c.generation,
            "age": w.time - c.born, "x": c.x, "y": c.y, "radius": c.radius,
            "biomass": c.biomass, "div_size": c.genome.div_size, "atp": c.atp,
            "atp_cap": w.params.atp_capacity * c.biomass, "damage": c.damage,
            "replication": c.replication, "dividing": c.dividing, "swim": c.swim,
            "temp": w.temp_at(gi), "light": w.light_at(gi, w.daylight()),
            "water": water, "t_opt": c.t_opt(), "growth": c.growth, "thermal": c.thermal,
            "ribo": c.expr_ribo, "internal": internal, "external": external,
            "flux": c.flux, "cap": c.cap.to_vec(), "strategy_mix": mix, "atp_rate": atp_rate,
            "color": c.color(),
        },
        "genome": {
            "ribo": c.genome.ribo, "div_size": c.genome.div_size, "hue": c.genome.hue,
            "mutations": c.genome.mutations, "codons": c.genome.total_codons, "genes": genes,
        },
    })
}
