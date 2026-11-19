//! `sim serve`: the engine runs natively in its own thread at an adjustable
//! speed; a browser page steers it and shows the town live.
//!
//! The page talks plain HTTP on localhost:
//!
//! | Request | Answer |
//! | --- | --- |
//! | `GET /` | the control page |
//! | `GET /api/layout` | map, buildings, traits (fixed) |
//! | `GET /api/state` | clock, speed, every agent, rents, prices |
//! | `GET /api/history?from=DAY` | daily history rows from DAY on |
//! | `GET /api/explain?agent=N` | how agent N scores its actions now |
//! | `POST /api/control?play=1&speed=N&seek=MIN&restart=1&seed=N` | change any of these |
//!
//! `speed` is game minutes per real second, or `max`. Speed only decides how
//! far the engine gets per second, never what happens: the state hash at a
//! given game minute is the same at any speed.

use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use timeloop_core::{Minute, MINUTES_PER_DAY};

use crate::live::Live;

const PAGE: &str = include_str!("live.html");
/// Longest stretch the engine holds the lock, so requests stay quick.
const SLICE: Duration = Duration::from_millis(10);

struct Shared {
    live: Live,
    playing: bool,
    /// Game minutes per real second; `None` is as fast as possible.
    speed: Option<u64>,
    /// Fractional minutes owed from earlier slices.
    carry: f64,
    /// Measured game minutes per real second.
    effective: f64,
    /// Seek in progress: run at full speed until this minute.
    target: Option<Minute>,
}

pub fn serve(live: Live, port: u16) -> io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let shared = Arc::new(Mutex::new(Shared {
        live,
        playing: false,
        speed: Some(60),
        carry: 0.0,
        effective: 0.0,
        target: None,
    }));
    {
        let shared = Arc::clone(&shared);
        thread::spawn(move || engine(&shared));
    }
    eprintln!("open http://127.0.0.1:{port}  (Ctrl+C to stop)");
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let shared = Arc::clone(&shared);
        thread::spawn(move || {
            let _ = handle(stream, &shared);
        });
    }
    Ok(())
}

/// Advances the world by real time × speed, in short slices.
fn engine(shared: &Mutex<Shared>) {
    let mut last = Instant::now();
    loop {
        let started = Instant::now();
        let dt = started.duration_since(last).as_secs_f64();
        last = started;
        let busy = {
            let mut s = shared.lock().expect("engine state");
            let end = s.target.unwrap_or(s.live.end()).min(s.live.end());
            if (s.playing || s.target.is_some()) && s.live.world().now() < end {
                let before = s.live.world().now();
                let mut want = match (s.target, s.speed) {
                    (Some(_), _) | (None, None) => f64::INFINITY,
                    (None, Some(v)) => v as f64 * dt + s.carry,
                };
                // Whole days at most per call, so the slice can end on time.
                while want >= 1.0 && started.elapsed() < SLICE && s.live.world().now() < end {
                    let left = (end - s.live.world().now()) as f64;
                    let step = want.min(MINUTES_PER_DAY as f64).min(left) as Minute;
                    s.live.advance(step);
                    want -= step as f64;
                }
                // Too slow to keep up: drop the backlog instead of piling it up.
                s.carry = if started.elapsed() >= SLICE {
                    0.0
                } else {
                    want.min(1.0)
                };
                let done = (s.live.world().now() - before) as f64;
                s.effective = 0.9 * s.effective + 0.1 * done / dt.max(1e-3);
                // Reaching a seek target ends the seek; reaching the end stops play.
                if s.live.world().now() >= end && s.target.take().is_none() {
                    s.playing = false;
                }
                true
            } else {
                s.target = None;
                s.effective *= 0.9;
                false
            }
        };
        thread::sleep(if busy {
            Duration::from_millis(2)
        } else {
            Duration::from_millis(20)
        });
    }
}

fn handle(stream: TcpStream, shared: &Mutex<Shared>) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    // Skip headers; requests carry no body we need.
    let mut header = String::new();
    while reader.read_line(&mut header)? > 2 {
        header.clear();
    }
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let param = |key: &str| {
        query
            .split('&')
            .filter_map(|kv| kv.split_once('='))
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v)
    };
    let number = |key: &str| param(key).and_then(|v| v.parse::<u64>().ok());

    let (status, kind, body) = match (method, path) {
        ("GET", "/") => ("200 OK", "text/html; charset=utf-8", PAGE.to_string()),
        ("GET", "/api/layout") => ("200 OK", JSON, lock(shared).live.layout_json()),
        ("GET", "/api/state") => ("200 OK", JSON, state(&lock(shared))),
        ("GET", "/api/history") => {
            let from = number("from").unwrap_or(0) as usize;
            ("200 OK", JSON, lock(shared).live.history_json(from))
        }
        ("GET", "/api/explain") => {
            let agent = number("agent").unwrap_or(u64::MAX).min(u32::MAX as u64) as u32;
            ("200 OK", JSON, lock(shared).live.explain_json(agent))
        }
        ("POST", "/api/control") => {
            let mut s = lock(shared);
            if param("restart").is_some() {
                let seed = number("seed");
                s.live.restart(seed);
            }
            match param("speed") {
                Some("max") => s.speed = None,
                Some(v) => {
                    if let Ok(v) = v.parse::<u64>() {
                        s.speed = Some(v.max(1));
                    }
                }
                None => {}
            }
            if let Some(to) = number("seek") {
                // Back to the year before at once; the engine runs the rest.
                s.live.rewind(to);
                s.target = Some(to.min(s.live.end()));
                s.carry = 0.0;
            }
            if let Some(play) = param("play") {
                s.playing = play == "1";
            }
            ("200 OK", JSON, state(&s))
        }
        _ => ("404 Not Found", "text/plain", "not found\n".to_string()),
    };
    let mut out = stream;
    write!(
        out,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    out.write_all(body.as_bytes())?;
    out.flush()
}

const JSON: &str = "application/json";

fn lock(shared: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    shared.lock().expect("engine state")
}

fn state(s: &Shared) -> String {
    let speed = s.speed.map_or("\"max\"".to_string(), |v| v.to_string());
    let seeking = s.target.map_or("null".to_string(), |t| t.to_string());
    format!(
        "{{\"playing\":{},\"speed\":{speed},\"effective\":{:.0},\"seeking\":{seeking},\"end\":{},\"world\":{}}}",
        s.playing,
        s.effective,
        s.live.end(),
        s.live.state_json()
    )
}
