//! Primordia: an open-ended cellular evolution simulator.
//!
//! Run `cargo run --release` and open the printed localhost address.
//! `--headless` runs without the browser UI and prints statistics.

mod cell;
mod chem;
mod genome;
mod params;
mod protocol;
mod rng;
mod server;
mod stats;
mod world;

use params::Params;
use world::World;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let flag = |name: &str| args.iter().any(|a| a == name);
    if flag("--help") || flag("-h") {
        println!("Primordia — open-ended cellular evolution\n");
        println!("usage: primordia [--port N] [--seed N] [--no-open] [--headless --steps N] [param=value ...]\n");
        println!("  --port N      port for the local web interface (default 4747)");
        println!("  --seed N      world seed (default: random)");
        println!("  --no-open     don't open the browser automatically");
        println!("  --headless    run without the interface, printing statistics");
        println!("  --steps N     steps to run in headless mode (default 36000 = 10 simulated minutes)");
        println!("  --params      list every adjustable parameter");
        println!("  key=value     override any parameter, e.g. atm_o2=1.5");
        return;
    }
    let mut params = Params::default();
    if flag("--params") {
        for d in Params::descriptors() {
            println!("{:24} {:>10} {:10} [{} .. {}]  {}", d.key, d.default, d.unit, d.min, d.max, d.label);
        }
        return;
    }
    for a in args.iter().skip(1).filter(|a| a.contains('=')) {
        let (k, v) = a.split_once('=').unwrap();
        match v.parse::<f32>() {
            Ok(v) if params.set(k, v) => {}
            _ => eprintln!("ignoring unknown parameter or bad value: {a}"),
        }
    }
    let seed: u64 = get("--seed").and_then(|s| s.parse().ok()).unwrap_or_else(server::random_seed);

    if flag("--headless") {
        headless(params, seed, get("--steps").and_then(|s| s.parse().ok()).unwrap_or(36000));
        return;
    }
    let port: u16 = get("--port").and_then(|s| s.parse().ok()).unwrap_or(4747);
    println!("Creating world (seed {seed})...");
    let world = World::new(params, seed);
    server::serve(world, port, !flag("--no-open"));
}

fn headless(params: Params, seed: u64, steps: u64) {
    let t0 = std::time::Instant::now();
    let mut w = World::new(params, seed);
    eprintln!("seed {seed}: world created in {:?}: {}x{} grid, {} vents", t0.elapsed(), w.gw, w.gh, w.vents.len());
    let t0 = std::time::Instant::now();
    let mut last = std::time::Instant::now();
    let mut seen_events = 0;
    for i in 0..steps {
        w.step();
        if i % 1800 == 0 {
            let l = w.stats.latest.clone().unwrap();
            let n = w.cells.len().max(1) as f32;
            let avg = |f: &dyn Fn(&cell::Cell) -> f32| w.cells.iter().map(f).sum::<f32>() / n;
            let sps = if i == 0 { 0.0 } else { 1800.0 / last.elapsed().as_secs_f32() };
            last = std::time::Instant::now();
            println!(
                "t={:7.0}s pop={:5} gen={:4} genes={:5.2} size={:5.1} topt={:4.1} atp={:.2} dmg={:.3} growth={:.4} | births={} deaths={} lineages={} | {:.0} steps/s",
                l.t,
                l.pop,
                l.max_gen,
                l.genes,
                l.size,
                l.t_opt,
                avg(&|c| c.atp / c.biomass),
                avg(&|c| c.damage),
                avg(&|c| c.growth),
                l.births,
                l.deaths,
                l.lineages,
                sps
            );
            let s: Vec<String> = l.strat.iter().map(|v| v.to_string()).collect();
            let c: Vec<String> = l.chem.iter().map(|v| format!("{:.0}", v)).collect();
            println!("         strategies=[{}] chem=[{}]", s.join(","), c.join(","));
            for e in w.stats.events.iter().filter(|e| e.seq > seen_events) {
                println!("  EVENT t={:.0}s {}", e.t, e.text);
            }
            seen_events = w.stats.event_total;
        }
    }
    let pr = w.prof;
    eprintln!(
        "{} steps in {:?}; per step: physics {:.3} ms, biology {:.3} ms, grid {:.3} ms",
        steps,
        t0.elapsed(),
        pr[0] * 1e3 / steps as f64,
        pr[1] * 1e3 / steps as f64,
        pr[2] * 1e3 / steps as f64
    );
}
