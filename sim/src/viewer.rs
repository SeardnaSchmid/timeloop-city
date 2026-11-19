//! Self-contained HTML map viewer: the town grid with its buildings and
//! every agent, recorded every few game minutes and played back in the
//! browser. No server, no external files.

use std::fmt::Write as _;
use std::io;
use std::path::Path;

use timeloop_core::{ActionKind, Minute, Pos, World, MINUTES_PER_DAY};

const TEMPLATE: &str = include_str!("viewer.html");

/// Action names in the order of [`ActionKind`] discriminants.
const ACTIONS: [&str; 7] = [
    "idle", "eat", "sleep", "work", "overtime", "wander", "travel",
];

/// Frames the automatic step aims for: enough to watch, small enough to load.
const TARGET_FRAMES: Minute = 4000;
/// Steps below a day divide the day evenly, so frames keep their clock time.
const STEPS: [Minute; 11] = [10, 15, 20, 30, 60, 120, 180, 240, 360, 480, 720];

pub struct Recorder {
    /// Game minutes between frames.
    step: Minute,
    next: Minute,
    end: Minute,
    frames: String,
    count: usize,
}

impl Recorder {
    /// Records `span` minutes from `start`, every `step` minutes or, without
    /// one, a step that gives about [`TARGET_FRAMES`] frames.
    pub fn new(start: Minute, span: Minute, step: Option<Minute>) -> Self {
        let step = step.unwrap_or_else(|| {
            let ideal = span.div_ceil(TARGET_FRAMES);
            STEPS
                .into_iter()
                .find(|&s| s >= ideal)
                .unwrap_or_else(|| ideal.div_ceil(MINUTES_PER_DAY) * MINUTES_PER_DAY)
        });
        let step = step.max(1);
        // Daily or longer: show the town at noon, when people are at work.
        let first = if step >= MINUTES_PER_DAY {
            start + MINUTES_PER_DAY / 2
        } else {
            start
        };
        Self {
            step,
            next: first,
            end: start + span,
            frames: String::new(),
            count: 0,
        }
    }

    /// When the next frame is due, if any is left.
    pub fn next_frame(&self) -> Option<Minute> {
        (self.next <= self.end).then_some(self.next)
    }

    /// Appends the world as it is now and moves on to the next frame.
    pub fn record(&mut self, w: &World) {
        self.next += self.step;
        let s = w.stats();
        let f = &mut self.frames;
        if self.count > 0 {
            f.push(',');
        }
        self.count += 1;
        let _ = write!(f, "{{\"t\":{},\"a\":[", w.now());
        for a in 0..w.agent_count() {
            let (x, y) = position_tenths(w, a);
            let action = w.action(a);
            if a > 0 {
                f.push(',');
            }
            let _ = write!(
                f,
                "{x},{y},{},{},{},{},{}",
                action.kind as u8,
                w.hunger(a),
                w.money(a),
                w.home(a).map_or(-1, |h| h as i64),
                w.employer(a).map_or(-1, |b| b as i64),
            );
        }
        let _ = write!(
            f,
            "],\"rent\":{:?},\"ten\":{:?},\"work\":{:?},\"price\":{:?},\"emp\":{},\"homeless\":{},\"gini\":{}}}",
            s.rents, s.tenants, s.workers, s.prices, s.employed, s.homeless, s.money_gini_permille
        );
    }

    pub fn write(&self, w: &World, path: &Path) -> io::Result<()> {
        let c = w.config();
        let m = &c.map;
        let mut data = String::new();
        let _ = write!(
            data,
            "{{\"w\":{},\"h\":{},\"step\":{},\"actions\":{},\"businesses\":[",
            m.width,
            m.height,
            self.step,
            json_strings(ACTIONS.iter().copied())
        );
        for (i, b) in c.businesses.iter().enumerate() {
            if i > 0 {
                data.push(',');
            }
            let _ = write!(
                data,
                "{{\"name\":{},\"good\":{},\"x\":{},\"y\":{},\"slots\":{}}}",
                json_string(&b.name),
                json_string(&c.goods[b.output as usize]),
                b.pos.x,
                b.pos.y,
                b.slots
            );
        }
        data.push_str("],\"houses\":[");
        for (i, h) in c.houses.iter().enumerate() {
            if i > 0 {
                data.push(',');
            }
            let _ = write!(
                data,
                "{{\"name\":{},\"x\":{},\"y\":{},\"beds\":{}}}",
                json_string(&h.name),
                h.pos.x,
                h.pos.y,
                h.beds
            );
        }
        data.push_str("],\"traits\":");
        data.push_str(&json_strings(c.traits.iter().map(String::as_str)));
        data.push_str(",\"agentTraits\":[");
        for a in 0..w.agent_count() {
            if a > 0 {
                data.push(',');
            }
            let _ = write!(data, "{:?}", w.traits(a));
        }
        data.push_str("],\"land\":[");
        for y in 0..m.height {
            for x in 0..m.width {
                if x > 0 || y > 0 {
                    data.push(',');
                }
                let _ = write!(data, "{}", w.land_price(Pos::new(x, y)));
            }
        }
        let _ = write!(data, "],\"frames\":[{}]}}", self.frames);
        // `</` must not appear inside the inline script.
        let data = data.replace("</", "<\\/");
        std::fs::write(path, TEMPLATE.replace("/*DATA*/null", &data))
    }
}

/// Where the agent is, in tenths of a parcel; travellers are placed along
/// their way (x first, then y), the same path the core uses.
fn position_tenths(w: &World, agent: u32) -> (u32, u32) {
    let p = w.pos(agent);
    let a = w.action(agent);
    if a.kind != ActionKind::Travel {
        return (p.x as u32 * 10, p.y as u32 * 10);
    }
    let rate = w.config().map.travel_minutes_per_parcel.max(1);
    let tenths = (w.now() - a.started) * 10 / rate;
    let (whole, frac) = ((tenths / 10) as u32, (tenths % 10) as i64);
    let (p1, p2) = (p.toward(a.target, whole), p.toward(a.target, whole + 1));
    let lerp = |a: u16, b: u16| (a as i64 * 10 + (b as i64 - a as i64) * frac) as u32;
    (lerp(p1.x, p2.x), lerp(p1.y, p2.y))
}

fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_strings<'a>(items: impl Iterator<Item = &'a str>) -> String {
    let parts: Vec<String> = items.map(json_string).collect();
    format!("[{}]", parts.join(","))
}
