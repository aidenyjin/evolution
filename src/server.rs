//! Local web server: runs the simulation on its own thread and streams it to
//! the browser over a WebSocket. The UI files are embedded in the binary.

use crate::params::Params;
use crate::protocol;
use crate::world::{Death, World};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{oneshot, watch};

const FILES: &[(&str, &str, &str)] = &[
    ("index.html", "text/html; charset=utf-8", include_str!("../web/index.html")),
    ("style.css", "text/css; charset=utf-8", include_str!("../web/style.css")),
    ("js/main.js", "text/javascript; charset=utf-8", include_str!("../web/js/main.js")),
    ("js/net.js", "text/javascript; charset=utf-8", include_str!("../web/js/net.js")),
    ("js/renderer.js", "text/javascript; charset=utf-8", include_str!("../web/js/renderer.js")),
    ("js/ui.js", "text/javascript; charset=utf-8", include_str!("../web/js/ui.js")),
    ("js/charts.js", "text/javascript; charset=utf-8", include_str!("../web/js/charts.js")),
    ("js/inspector.js", "text/javascript; charset=utf-8", include_str!("../web/js/inspector.js")),
    ("js/editor.js", "text/javascript; charset=utf-8", include_str!("../web/js/editor.js")),
];

#[derive(Clone, Copy, PartialEq)]
pub enum Speed {
    /// Simulate n steps per drawn frame, at 60 frames per second.
    Every(u32),
    /// Simulate as fast as possible, drawing ~30 frames per second.
    Max,
}

impl Speed {
    fn json(&self) -> Value {
        match self {
            Speed::Every(n) => json!({"mode": "every", "n": n}),
            Speed::Max => json!({"mode": "max"}),
        }
    }
}

pub enum Cmd {
    Pause,
    Resume,
    Step(u32),
    Speed(Speed),
    Set(String, f32),
    ResetParams,
    NewWorld(Option<u64>),
    Info(oneshot::Sender<(String, Vec<u8>)>),
    Inspect(u64, oneshot::Sender<String>),
    History(oneshot::Sender<String>),
    Save(Option<String>, oneshot::Sender<String>),
    Load(String, oneshot::Sender<String>),
    ListSaves(oneshot::Sender<String>),
    /// Fold codon strings into proteins (editor previews).
    Fold(Vec<Value>, oneshot::Sender<String>),
    /// Design a gene for a function: (function, temperature, length, location).
    Design(usize, f32, usize, String, oneshot::Sender<String>),
    Template(String, oneshot::Sender<String>),
    CellSpec(u64, oneshot::Sender<String>),
    EditCell(u64, Value, oneshot::Sender<String>),
    /// Place new cells: (spec, x, y, count, spread).
    Spawn(Value, f32, f32, usize, f32, oneshot::Sender<String>),
    DeleteCell(u64, oneshot::Sender<String>),
}

#[derive(Clone)]
struct AppState {
    cmd: mpsc::Sender<Cmd>,
    frames: watch::Receiver<Arc<Vec<u8>>>,
    grid: watch::Receiver<Arc<Vec<u8>>>,
    stats: watch::Receiver<Arc<String>>,
    params: watch::Receiver<Arc<String>>,
    info_ver: watch::Receiver<u64>,
    statics: watch::Receiver<Arc<Vec<u8>>>,
}

struct Outputs {
    frames: watch::Sender<Arc<Vec<u8>>>,
    grid: watch::Sender<Arc<Vec<u8>>>,
    stats: watch::Sender<Arc<String>>,
    params: watch::Sender<Arc<String>>,
    info_ver: watch::Sender<u64>,
    statics: watch::Sender<Arc<Vec<u8>>>,
}

fn saves_dir() -> std::path::PathBuf {
    std::path::PathBuf::from("saves")
}

fn sanitize(name: &str) -> String {
    let s: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == ' ').collect();
    let s = s.trim().replace(' ', "_");
    if s.is_empty() { "world".into() } else { s.chars().take(60).collect() }
}

fn toast(text: impl Into<String>, ok: bool) -> String {
    json!({"type": "toast", "text": text.into(), "ok": ok}).to_string()
}

fn params_json(p: &Params) -> String {
    let values: serde_json::Map<String, Value> = p.values().into_iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
    json!({"type": "params", "values": values}).to_string()
}

fn run_sim(mut world: World, rx: mpsc::Receiver<Cmd>, out: Outputs) {
    let mut paused = false;
    let mut speed = Speed::Every(1);
    let mut pending_steps: u32 = 0;
    let frame_dt = Duration::from_secs_f64(1.0 / 60.0);
    let mut next_frame = Instant::now();
    let mut last_grid = Instant::now() - Duration::from_secs(1);
    let mut last_stats = Instant::now() - Duration::from_secs(1);
    let mut last_static = Instant::now();
    let mut sps_t0 = Instant::now();
    let mut sps_count: u64 = 0;
    let mut sps = 0.0f32;
    let mut info_ver = 0u64;
    let mut deaths: Vec<Death> = Vec::new();
    let mut force_publish = true;

    loop {
        // Commands from the browser.
        loop {
            let cmd = match rx.try_recv() {
                Ok(c) => c,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return,
            };
            match cmd {
                Cmd::Pause => paused = true,
                Cmd::Resume => {
                    paused = false;
                    next_frame = Instant::now();
                }
                Cmd::Step(n) => pending_steps += n.min(100_000),
                Cmd::Speed(s) => {
                    speed = s;
                    next_frame = Instant::now();
                }
                Cmd::Set(k, v) => {
                    if world.params.set(&k, v) {
                        world.params_changed();
                        let _ = out.params.send(Arc::new(params_json(&world.params)));
                    }
                }
                Cmd::ResetParams => {
                    world.params = Params::default();
                    world.params_changed();
                    let _ = out.params.send(Arc::new(params_json(&world.params)));
                }
                Cmd::NewWorld(seed) => {
                    let seed = seed.unwrap_or_else(random_seed);
                    world = World::new(world.params.clone(), seed);
                    deaths.clear();
                    info_ver += 1;
                    let _ = out.info_ver.send(info_ver);
                    force_publish = true;
                }
                Cmd::Info(reply) => {
                    let info = protocol::info_json(&world, paused, &speed.json()).to_string();
                    let _ = reply.send((info, protocol::encode_static(&world)));
                }
                Cmd::Inspect(id, reply) => {
                    let _ = reply.send(protocol::inspect_json(&world, id).to_string());
                }
                Cmd::History(reply) => {
                    let _ = reply.send(json!({"type": "history", "history": world.stats.history, "hist_version": world.stats.hist_version}).to_string());
                }
                Cmd::Save(name, reply) => {
                    let name = sanitize(&name.unwrap_or_else(|| format!("seed{}-t{}", world.seed, world.time as u64)));
                    let dir = saves_dir();
                    let res = std::fs::create_dir_all(&dir)
                        .map_err(|e| e.to_string())
                        .and_then(|_| bincode::serialize(&world).map_err(|e| e.to_string()))
                        .and_then(|bytes| std::fs::write(dir.join(format!("{name}.primordia")), bytes).map_err(|e| e.to_string()));
                    let _ = reply.send(match res {
                        Ok(_) => toast(format!("Saved \"{name}\""), true),
                        Err(e) => toast(format!("Save failed: {e}"), false),
                    });
                }
                Cmd::Load(name, reply) => {
                    let path = saves_dir().join(format!("{}.primordia", sanitize(&name)));
                    let res = std::fs::read(&path)
                        .map_err(|e| e.to_string())
                        .and_then(|b| bincode::deserialize::<World>(&b).map_err(|e| e.to_string()));
                    let msg = match res {
                        Ok(mut w) => {
                            w.rebuild();
                            w.params_changed();
                            world = w;
                            deaths.clear();
                            info_ver += 1;
                            let _ = out.info_ver.send(info_ver);
                            force_publish = true;
                            toast(format!("Loaded \"{name}\""), true)
                        }
                        Err(e) => toast(format!("Load failed: {e}"), false),
                    };
                    let _ = reply.send(msg);
                }
                Cmd::Fold(seqs, reply) => {
                    let _ = reply.send(crate::editor::fold_many(&seqs).to_string());
                }
                Cmd::Design(f, t_opt, len, loc, reply) => {
                    let seed = world.rng.next_u64();
                    let _ = reply.send(crate::editor::design(f, t_opt, len, &loc, seed).to_string());
                }
                Cmd::Template(name, reply) => {
                    let _ = reply.send(crate::editor::template(&name).to_string());
                }
                Cmd::CellSpec(id, reply) => {
                    let _ = reply.send(crate::editor::cell_spec(&world, id).to_string());
                }
                Cmd::EditCell(id, spec, reply) => {
                    let r = world.edit_cell(id, &spec);
                    force_publish = true;
                    let _ = reply.send(match r {
                        Ok(t) => toast(t, true),
                        Err(e) => toast(e, false),
                    });
                }
                Cmd::Spawn(spec, x, y, count, spread, reply) => {
                    let r = world.spawn_cells(&spec, x, y, count, spread);
                    force_publish = true;
                    let _ = reply.send(match r {
                        Ok(t) => toast(t, true),
                        Err(e) => toast(e, false),
                    });
                }
                Cmd::DeleteCell(id, reply) => {
                    let r = world.delete_cell(id);
                    force_publish = true;
                    let _ = reply.send(match r {
                        Ok(t) => toast(t, true),
                        Err(e) => toast(e, false),
                    });
                }
                Cmd::ListSaves(reply) => {
                    let mut files = Vec::new();
                    if let Ok(rd) = std::fs::read_dir(saves_dir()) {
                        for e in rd.flatten() {
                            let p = e.path();
                            if p.extension().and_then(|x| x.to_str()) == Some("primordia") {
                                let meta = e.metadata().ok();
                                let modified = meta
                                    .as_ref()
                                    .and_then(|m| m.modified().ok())
                                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                    .map(|d| d.as_secs())
                                    .unwrap_or(0);
                                files.push(json!({
                                    "name": p.file_stem().and_then(|s| s.to_str()).unwrap_or(""),
                                    "size": meta.map(|m| m.len()).unwrap_or(0),
                                    "modified": modified,
                                }));
                            }
                        }
                    }
                    files.sort_by_key(|f| std::cmp::Reverse(f["modified"].as_u64().unwrap_or(0)));
                    let _ = reply.send(json!({"type": "saves", "files": files}).to_string());
                }
            }
        }

        // Advance the simulation according to the speed mode.
        let now = Instant::now();
        let mut stepped = 0u32;
        if !paused {
            match speed {
                Speed::Every(n) => {
                    if now >= next_frame {
                        for _ in 0..n {
                            world.step();
                        }
                        stepped = n;
                        next_frame += frame_dt;
                        if next_frame + Duration::from_millis(250) < now {
                            next_frame = now + frame_dt;
                        }
                    }
                }
                Speed::Max => {
                    let t0 = Instant::now();
                    while t0.elapsed() < Duration::from_millis(30) {
                        world.step();
                        stepped += 1;
                    }
                }
            }
        } else if pending_steps > 0 {
            let t0 = Instant::now();
            while pending_steps > 0 && t0.elapsed() < Duration::from_millis(30) {
                world.step();
                pending_steps -= 1;
                stepped += 1;
            }
        }
        if paused && pending_steps == 0 {
            // Unpausing restarts real-time pacing from now.
            next_frame = Instant::now();
        }
        sps_count += stepped as u64;

        if stepped > 0 || force_publish {
            deaths.extend(world.take_deaths());
            let frame = protocol::encode_cells(&world, &deaths);
            deaths.clear();
            let _ = out.frames.send(Arc::new(frame));
        }
        let now = Instant::now();
        if (stepped > 0 && now - last_grid > Duration::from_millis(90)) || force_publish {
            last_grid = now;
            let _ = out.grid.send(Arc::new(protocol::encode_grid(&world)));
        }
        let el = (now - sps_t0).as_secs_f32();
        if el >= 1.0 {
            sps = sps_count as f32 / el;
            sps_count = 0;
            sps_t0 = now;
        }
        // Vent heat changes as vents age, so resend the heat map now and then.
        if stepped > 0 && world.params.vent_lifetime > 0.0 && now - last_static > Duration::from_millis(1500) {
            last_static = now;
            let _ = out.statics.send(Arc::new(protocol::encode_static(&world)));
        }
        if now - last_stats > Duration::from_millis(400) || force_publish {
            last_stats = now;
            let _ = out.stats.send(Arc::new(protocol::stats_json(&world, sps, paused, &speed.json()).to_string()));
        }
        force_publish = false;

        // Idle until there is something to do.
        if paused && pending_steps == 0 {
            std::thread::sleep(Duration::from_millis(8));
        } else if let Speed::Every(_) = speed {
            let now = Instant::now();
            if next_frame > now {
                let wait = next_frame - now;
                std::thread::sleep(wait.min(Duration::from_millis(4)));
            }
        }
    }
}

pub fn random_seed() -> u64 {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
    (t ^ (t >> 17)).wrapping_mul(0x9E37_79B9_7F4A_7C15) % 1_000_000
}

pub fn serve(world: World, port: u16, open_browser: bool) {
    let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
    let (frames_tx, frames_rx) = watch::channel(Arc::new(Vec::new()));
    let (grid_tx, grid_rx) = watch::channel(Arc::new(Vec::new()));
    let (stats_tx, stats_rx) = watch::channel(Arc::new(String::new()));
    let (params_tx, params_rx) = watch::channel(Arc::new(params_json(&world.params)));
    let (info_tx, info_rx) = watch::channel(0u64);
    let (statics_tx, statics_rx) = watch::channel(Arc::new(Vec::new()));
    let out = Outputs { frames: frames_tx, grid: grid_tx, stats: stats_tx, params: params_tx, info_ver: info_tx, statics: statics_tx };
    std::thread::Builder::new().name("simulation".into()).spawn(move || run_sim(world, cmd_rx, out)).expect("spawn simulation");

    let state = AppState { cmd: cmd_tx, frames: frames_rx, grid: grid_rx, stats: stats_rx, params: params_rx, info_ver: info_rx, statics: statics_rx };
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("tokio runtime");
    rt.block_on(async move {
        let app = Router::new()
            .route("/", get(|| async { file_response("index.html") }))
            .route("/ws", get(ws_handler))
            .route("/*path", get(|Path(path): Path<String>| async move { file_response(&path) }))
            .with_state(state);
        let mut listener = None;
        let mut bound = port;
        for p in port..port + 20 {
            if let Ok(l) = tokio::net::TcpListener::bind(("127.0.0.1", p)).await {
                listener = Some(l);
                bound = p;
                break;
            }
        }
        let Some(listener) = listener else {
            eprintln!("Could not bind to any port between {} and {}", port, port + 19);
            return;
        };
        let url = format!("http://localhost:{bound}");
        println!();
        println!("  ┌──────────────────────────────────────────────┐");
        println!("  │  Primordia is running                        │");
        println!("  │  Open {url:<39}│");
        println!("  │  Press Ctrl+C to stop                        │");
        println!("  └──────────────────────────────────────────────┘");
        println!();
        if open_browser {
            let opener = if cfg!(target_os = "macos") { "open" } else if cfg!(target_os = "windows") { "explorer" } else { "xdg-open" };
            let _ = std::process::Command::new(opener).arg(&url).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
        }
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("server error: {e}");
        }
    });
}

fn file_response(path: &str) -> Response {
    match FILES.iter().find(|(p, _, _)| *p == path) {
        Some((_, mime, body)) => ([(header::CONTENT_TYPE, *mime), (header::CACHE_CONTROL, "no-cache")], *body).into_response(),
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

async fn ws_handler(ws: WebSocketUpgrade, State(st): State<AppState>) -> Response {
    ws.max_message_size(1 << 20).on_upgrade(move |socket| client(socket, st))
}

async fn request<T>(st: &AppState, make: impl FnOnce(oneshot::Sender<T>) -> Cmd) -> Option<T> {
    let (tx, rx) = oneshot::channel();
    st.cmd.send(make(tx)).ok()?;
    rx.await.ok()
}

async fn client(socket: WebSocket, st: AppState) {
    let (mut sink, mut stream) = socket.split();
    let mut frames = st.frames.clone();
    let mut grid = st.grid.clone();
    let mut stats = st.stats.clone();
    let mut params = st.params.clone();
    let mut info_ver = st.info_ver.clone();
    let mut statics = st.statics.clone();
    statics.borrow_and_update();
    info_ver.borrow_and_update();
    params.borrow_and_update();

    let Some((info, stat)) = request(&st, Cmd::Info).await else { return };
    if sink.send(Message::Text(info)).await.is_err() || sink.send(Message::Binary(stat)).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            r = frames.changed() => {
                if r.is_err() { break; }
                let f = frames.borrow_and_update().clone();
                if !f.is_empty() && sink.send(Message::Binary(f.as_ref().clone())).await.is_err() { break; }
            }
            r = grid.changed() => {
                if r.is_err() { break; }
                let g = grid.borrow_and_update().clone();
                if !g.is_empty() && sink.send(Message::Binary(g.as_ref().clone())).await.is_err() { break; }
            }
            r = stats.changed() => {
                if r.is_err() { break; }
                let s = stats.borrow_and_update().clone();
                if !s.is_empty() && sink.send(Message::Text(s.as_ref().clone())).await.is_err() { break; }
            }
            r = statics.changed() => {
                if r.is_err() { break; }
                let s = statics.borrow_and_update().clone();
                if !s.is_empty() && sink.send(Message::Binary(s.as_ref().clone())).await.is_err() { break; }
            }
            r = params.changed() => {
                if r.is_err() { break; }
                let s = params.borrow_and_update().clone();
                if sink.send(Message::Text(s.as_ref().clone())).await.is_err() { break; }
            }
            r = info_ver.changed() => {
                if r.is_err() { break; }
                info_ver.borrow_and_update();
                if let Some((info, stat)) = request(&st, Cmd::Info).await {
                    if sink.send(Message::Text(info)).await.is_err() || sink.send(Message::Binary(stat)).await.is_err() { break; }
                }
            }
            m = stream.next() => {
                match m {
                    Some(Ok(Message::Text(t))) => {
                        if let Some(reply) = handle_message(&st, &t).await {
                            if sink.send(Message::Text(reply)).await.is_err() { break; }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }
}

async fn handle_message(st: &AppState, text: &str) -> Option<String> {
    let v: Value = serde_json::from_str(text).ok()?;
    let cmd = v.get("cmd")?.as_str()?;
    let send = |c: Cmd| {
        let _ = st.cmd.send(c);
        None
    };
    let reply = match cmd {
        "pause" => send(Cmd::Pause),
        "resume" => send(Cmd::Resume),
        "step" => send(Cmd::Step(v.get("n").and_then(|n| n.as_u64()).unwrap_or(1) as u32)),
        "speed" => {
            let s = match v.get("mode").and_then(|m| m.as_str()) {
                Some("max") => Speed::Max,
                _ => Speed::Every(v.get("n").and_then(|n| n.as_u64()).unwrap_or(1).clamp(1, 1000) as u32),
            };
            send(Cmd::Speed(s))
        }
        "set" => {
            let key = v.get("key")?.as_str()?.to_string();
            let value = v.get("value")?.as_f64()? as f32;
            send(Cmd::Set(key, value))
        }
        "reset_params" => send(Cmd::ResetParams),
        "new_world" => send(Cmd::NewWorld(v.get("seed").and_then(|s| s.as_u64()))),
        "inspect" => request(st, |tx| Cmd::Inspect(v.get("id").and_then(|i| i.as_u64()).unwrap_or(0), tx)).await,
        "history" => request(st, Cmd::History).await,
        "save" => request(st, |tx| Cmd::Save(v.get("name").and_then(|n| n.as_str()).map(String::from), tx)).await,
        "load" => request(st, |tx| Cmd::Load(v.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string(), tx)).await,
        "list_saves" => request(st, Cmd::ListSaves).await,
        "fold" => request(st, |tx| Cmd::Fold(v.get("seqs").and_then(|s| s.as_array()).cloned().unwrap_or_default(), tx)).await,
        "design" => {
            let f = v.get("f").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
            let t = v.get("t_opt").and_then(|x| x.as_f64()).unwrap_or(48.0) as f32;
            let len = v.get("len").and_then(|x| x.as_u64()).unwrap_or(50) as usize;
            let loc = v.get("loc").and_then(|x| x.as_str()).unwrap_or("auto").to_string();
            request(st, |tx| Cmd::Design(f, t, len, loc, tx)).await
        }
        "template" => request(st, |tx| Cmd::Template(v.get("name").and_then(|n| n.as_str()).unwrap_or("luca").to_string(), tx)).await,
        "cell_spec" => request(st, |tx| Cmd::CellSpec(v.get("id").and_then(|i| i.as_u64()).unwrap_or(0), tx)).await,
        "edit_cell" => {
            let id = v.get("id").and_then(|i| i.as_u64()).unwrap_or(0);
            let spec = v.get("spec").cloned().unwrap_or(Value::Null);
            request(st, |tx| Cmd::EditCell(id, spec, tx)).await
        }
        "spawn" => {
            let spec = v.get("spec").cloned().unwrap_or(Value::Null);
            let num = |k: &str, d: f64| v.get(k).and_then(|x| x.as_f64()).unwrap_or(d);
            let (x, y, count, spread) = (num("x", 0.0) as f32, num("y", 0.0) as f32, num("count", 1.0) as usize, num("spread", 8.0) as f32);
            request(st, |tx| Cmd::Spawn(spec, x, y, count, spread, tx)).await
        }
        "delete_cell" => request(st, |tx| Cmd::DeleteCell(v.get("id").and_then(|i| i.as_u64()).unwrap_or(0), tx)).await,
        _ => None,
    };
    // Echo the request id so the browser can match replies to requests.
    match (reply, v.get("req")) {
        (Some(r), Some(req)) => match serde_json::from_str::<Value>(&r) {
            Ok(Value::Object(mut m)) => {
                m.insert("req".into(), req.clone());
                Some(Value::Object(m).to_string())
            }
            _ => Some(r),
        },
        (r, _) => r,
    }
}
