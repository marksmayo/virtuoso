//! What is on screen: every window's accessibility nodes, flattened in pre-order.

use serde_json::Value;

pub const CLICKABLE: u32 = 1;
pub const ENABLED: u32 = 2;
pub const FOCUSED: u32 = 4;
pub const SCROLLABLE: u32 = 8;
pub const CHECKED: u32 = 16;
pub const VISIBLE: u32 = 32;
pub const SELECTED: u32 = 64;
pub const EDITABLE: u32 = 128;
pub const CHECKABLE: u32 = 256;
pub const LONG_CLICKABLE: u32 = 512;
pub const PASSWORD: u32 = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }
    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
    pub fn center(&self) -> (i32, i32) {
        ((self.left + self.right) / 2, (self.top + self.bottom) / 2)
    }
    pub fn area(&self) -> i64 {
        self.width().max(0) as i64 * self.height().max(0) as i64
    }
    pub fn intersect(&self, o: &Rect) -> Rect {
        Rect {
            left: self.left.max(o.left),
            top: self.top.max(o.top),
            right: self.right.min(o.right),
            bottom: self.bottom.min(o.bottom),
        }
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Element {
    pub depth: u16,
    pub window: u16,
    pub bounds: Rect,
    pub text: String,
    pub desc: String,
    pub id: String,
    pub class: String,
    pub hint: String,
    pub error: String,
    pub flags: u32,
    pub parent: Option<usize>,
    pub package: String,
}

impl Element {
    pub fn has(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }
    pub fn clickable(&self) -> bool {
        self.has(CLICKABLE)
    }
    /// What a person reads on it: its text, else its description, else its hint.
    pub fn label(&self) -> &str {
        [&self.text, &self.desc, &self.hint].into_iter().find(|s| !s.is_empty()).map(String::as_str).unwrap_or("")
    }
}

#[derive(Debug, Clone, Default)]
pub struct ScreenTree {
    pub elements: Vec<Element>,
    pub width: i32,
    pub height: i32,
}

impl ScreenTree {
    /// Links each node to its parent: the closest node before it with a smaller depth.
    pub fn new(mut elements: Vec<Element>, width: i32, height: i32) -> Self {
        let mut stack: Vec<usize> = Vec::new();
        for i in 0..elements.len() {
            let d = elements[i].depth;
            while let Some(&top) = stack.last() {
                if elements[top].depth >= d {
                    stack.pop();
                } else {
                    break;
                }
            }
            elements[i].parent = if d == 0 { None } else { stack.last().copied() };
            stack.push(i);
        }
        ScreenTree { elements, width, height }
    }

    /// From the agent's `screen` reply, parsed into a `Value` (see [`ScreenReply::parse`] for
    /// the bytes).
    pub fn from_agent(reply: &Value, width: i32, height: i32) -> Self {
        let empty = vec![];
        let windows = reply["windows"].as_array().unwrap_or(&empty);
        let s = |v: &Value| v.as_str().unwrap_or("").to_string();
        let nodes = reply["nodes"].as_array().unwrap_or(&empty);
        let packages: Vec<String> = windows.iter().map(|w| s(&w["pkg"])).collect();
        let elements = nodes
            .iter()
            .map(|n| {
                let b = &n["b"];
                let w = n["w"].as_u64().unwrap_or(0) as usize;
                Element {
                    depth: n["d"].as_u64().unwrap_or(0) as u16,
                    window: w as u16,
                    bounds: Rect {
                        left: b[0].as_i64().unwrap_or(0) as i32,
                        top: b[1].as_i64().unwrap_or(0) as i32,
                        right: b[2].as_i64().unwrap_or(0) as i32,
                        bottom: b[3].as_i64().unwrap_or(0) as i32,
                    },
                    text: s(&n["t"]),
                    desc: s(&n["c"]),
                    id: s(&n["i"]),
                    class: s(&n["k"]),
                    hint: s(&n["h"]),
                    error: s(&n["e"]),
                    flags: n["f"].as_u64().unwrap_or(0) as u32,
                    parent: None,
                    package: packages.get(w).cloned().unwrap_or_default(),
                }
            })
            .collect();
        ScreenTree::new(elements, width, height)
    }

    pub fn screen_rect(&self) -> Rect {
        Rect { left: 0, top: 0, right: self.width, bottom: self.height }
    }

    /// Visible to the user and at least 10% on screen, as Maestro's filterOutOfBounds.
    pub fn matchable(&self, i: usize) -> bool {
        let e = &self.elements[i];
        if !e.has(VISIBLE) || e.bounds.area() == 0 {
            return false;
        }
        let on = e.bounds.intersect(&self.screen_rect()).area();
        on * 10 >= e.bounds.area()
    }

    /// The visible element a tap at (x, y) lands on: in the topmost window there (Android lists
    /// windows top first), the last in pre-order, which draws over its earlier siblings.
    pub fn element_at(&self, x: i32, y: i32) -> Option<usize> {
        let mut hits = (0..self.elements.len()).filter(|&i| self.elements[i].has(VISIBLE) && self.elements[i].bounds.contains(x, y));
        let top = hits.clone().map(|i| self.elements[i].window).min()?;
        hits.rfind(|&i| self.elements[i].window == top)
    }

    pub fn is_ancestor(&self, ancestor: usize, mut i: usize) -> bool {
        while let Some(p) = self.elements[i].parent {
            if p == ancestor {
                return true;
            }
            i = p;
        }
        false
    }

    /// A hash of what the screen shows (texts, ids, states and positions), to tell whether
    /// an action changed anything.
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut mix = |b: &[u8]| {
            for x in b {
                h ^= *x as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        };
        for e in &self.elements {
            mix(e.text.as_bytes());
            mix(e.desc.as_bytes());
            mix(e.id.as_bytes());
            mix(&e.flags.to_le_bytes());
            mix(&e.bounds.left.to_le_bytes());
            mix(&e.bounds.top.to_le_bytes());
            mix(&e.bounds.right.to_le_bytes());
            mix(&e.bounds.bottom.to_le_bytes());
        }
        h
    }
}

/// The agent's `screen` reply.
#[derive(Debug)]
pub struct ScreenReply {
    pub tree: ScreenTree,
    /// The agent's count of UI events when it read the screen; the next read may ask to be
    /// told when nothing has happened since.
    pub seq: Option<u64>,
    /// Nothing has happened since the read the request named (`tree` is then empty).
    pub unchanged: bool,
}

impl ScreenReply {
    /// Straight from the reply's bytes into elements: no `Value` tree in between, which would
    /// cost ~3000 allocations and ~220 KB for a 250-node screen.
    pub fn parse(body: &[u8], width: i32, height: i32) -> anyhow::Result<ScreenReply> {
        use std::borrow::Cow;
        #[derive(serde::Deserialize)]
        struct Reply<'a> {
            #[serde(default)]
            ok: bool,
            #[serde(default, borrow)]
            error: Option<Cow<'a, str>>,
            #[serde(default)]
            seq: Option<u64>,
            #[serde(default)]
            unchanged: bool,
            #[serde(default, borrow)]
            windows: Vec<Window<'a>>,
            #[serde(default, borrow)]
            nodes: Vec<Node<'a>>,
        }
        #[derive(serde::Deserialize)]
        struct Window<'a> {
            #[serde(default, borrow)]
            pkg: Cow<'a, str>,
        }
        #[derive(serde::Deserialize)]
        struct Node<'a> {
            #[serde(default)]
            d: u16,
            #[serde(default)]
            w: u16,
            #[serde(default)]
            b: [i32; 4],
            #[serde(default)]
            f: u32,
            #[serde(default, borrow)]
            t: Cow<'a, str>,
            #[serde(default, borrow)]
            c: Cow<'a, str>,
            #[serde(default, borrow)]
            i: Cow<'a, str>,
            #[serde(default, borrow)]
            k: Cow<'a, str>,
            #[serde(default, borrow)]
            h: Option<Cow<'a, str>>,
            #[serde(default, borrow)]
            e: Option<Cow<'a, str>>,
        }
        let r: Reply = serde_json::from_slice(body)?;
        if !r.ok {
            anyhow::bail!("agent: {}", r.error.as_deref().unwrap_or("request failed"));
        }
        let elements = r
            .nodes
            .into_iter()
            .map(|n| Element {
                depth: n.d,
                window: n.w,
                bounds: Rect { left: n.b[0], top: n.b[1], right: n.b[2], bottom: n.b[3] },
                text: n.t.into_owned(),
                desc: n.c.into_owned(),
                id: n.i.into_owned(),
                class: n.k.into_owned(),
                hint: n.h.map(Cow::into_owned).unwrap_or_default(),
                error: n.e.map(Cow::into_owned).unwrap_or_default(),
                flags: n.f,
                parent: None,
                package: r.windows.get(n.w as usize).map(|w| w.pkg.to_string()).unwrap_or_default(),
            })
            .collect();
        Ok(ScreenReply { tree: ScreenTree::new(elements, width, height), seq: r.seq, unchanged: r.unchanged })
    }
}
