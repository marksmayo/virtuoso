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
/// In a keyboard's window (an input method's): set from the window, not sent per node.
pub const KEYBOARD: u32 = 2048;
/// AccessibilityWindowInfo.TYPE_INPUT_METHOD.
pub const WINDOW_TYPE_IME: i64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// As Android writes bounds: `[left,top][right,bottom]`.
impl std::fmt::Display for Rect {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "[{},{}][{},{}]", self.left, self.top, self.right, self.bottom)
    }
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
    /// Its kind (`Button`, `android.widget.TextView`): one shared string per kind in a read.
    pub class: std::sync::Arc<str>,
    pub hint: String,
    pub error: String,
    pub flags: u32,
    pub parent: Option<usize>,
    /// Its app: one shared string per window.
    pub package: std::sync::Arc<str>,
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
    /// Shared, never changed after a read: a tree kept for the next command's first look, or
    /// handed to it, is the same tree, not a copy of every element.
    pub elements: std::sync::Arc<[Element]>,
    pub width: i32,
    pub height: i32,
    /// Which read it is, as the agent can tell it again (iOS: a hash of the whole tree): a
    /// wait asks for a read only when the screen is no longer this one.
    pub seq: Option<u64>,
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
        ScreenTree { elements: elements.into(), width, height, seq: None }
    }

    /// From the agent's `screen` reply, parsed into a `Value` (see [`ScreenReply::parse`] for
    /// the bytes).
    pub fn from_agent(reply: &Value, width: i32, height: i32) -> Self {
        let empty = vec![];
        let windows = reply["windows"].as_array().unwrap_or(&empty);
        let s = |v: &Value| v.as_str().unwrap_or("").to_string();
        let nodes = reply["nodes"].as_array().unwrap_or(&empty);
        let packages: Vec<String> = windows.iter().map(|w| s(&w["pkg"])).collect();
        let keyboard = |w: usize| windows.get(w).is_some_and(|w| w["type"].as_i64() == Some(WINDOW_TYPE_IME));
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
                    class: s(&n["k"]).into(),
                    hint: s(&n["h"]),
                    error: s(&n["e"]),
                    flags: n["f"].as_u64().unwrap_or(0) as u32 | if keyboard(w) { KEYBOARD } else { 0 },
                    parent: None,
                    package: packages.get(w).map(|p| p.as_str().into()).unwrap_or_default(),
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

    /// The list a sideways scroll drags across, as speedrun picks it: of the scroll views that
    /// scroll sideways (a horizontal view or pager by its class, or one whose items sit side by
    /// side), the one nearest the screen's centre. A list away from the centre (a row of cards
    /// at the top) never moves under a drag across the middle.
    pub fn sideways_list(&self) -> Option<usize> {
        let (cx, cy) = (self.width / 2, self.height / 2);
        let off_centre = |b: &Rect| (b.left - cx).max(cx - b.right).max(0) + (b.top - cy).max(cy - b.bottom).max(0);
        (0..self.elements.len())
            .filter(|&k| self.elements[k].has(SCROLLABLE) && self.elements[k].bounds.area() > 0 && self.scrolls_sideways(k))
            .min_by_key(|&k| off_centre(&self.elements[k].bounds))
    }

    /// The nearest list around element `i` that scrolls up and down.
    pub fn vertical_list_around(&self, mut i: usize) -> Option<usize> {
        while let Some(p) = self.elements[i].parent {
            if self.elements[p].has(SCROLLABLE) && !self.scrolls_sideways(p) {
                return Some(p);
            }
            i = p;
        }
        None
    }

    fn scrolls_sideways(&self, k: usize) -> bool {
        let class = &self.elements[k].class;
        if class.contains("HorizontalScrollView") || class.contains("ViewPager") {
            return true;
        }
        // Android names its vertical scroll views; iOS calls every one ScrollView.
        if &**class != "ScrollView" && (class.ends_with("ScrollView") || class.ends_with("ListView")) {
            return false;
        }
        // The items, past a content view that holds them all (an iOS scroll view's bars are
        // children of its own, not items).
        let bar = |c: usize| self.elements[c].text.contains("scroll bar");
        let children = move |p: usize| (0..self.elements.len()).filter(move |&c| self.elements[c].parent == Some(p) && !bar(c));
        let mut holder = k;
        for _ in 0..3 {
            let mut c = children(holder);
            match (c.next(), c.next()) {
                (Some(only), None) => holder = only,
                _ => break,
            }
        }
        let mut items = children(holder).map(|c| self.elements[c].bounds);
        match (items.next(), items.next()) {
            (Some(a), Some(b)) => b.left >= a.right || b.right <= a.left,
            _ => false,
        }
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
        for e in self.elements.iter() {
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
    /// A settle's outcome (iOS): the screen changed, or it never held still in time.
    pub changed: bool,
    pub timed_out: bool,
    /// The reply carried a screen (a settle's: the one it ended on).
    pub has_screen: bool,
    /// A settle's trace (iOS): idle waits ms, snapshots, snapshot ms.
    pub settle_trace: (u64, u64, u64),
    /// The snapshot fetched only the attributes a screen uses (iOS).
    pub trimmed: bool,
    /// Where the agent's time went (iOS): finding the app in front, snapshots, walks.
    pub trace: String,
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
            #[serde(default)]
            changed: bool,
            #[serde(default, rename = "timedOut")]
            timed_out: bool,
            #[serde(default, rename = "idleMs")]
            idle_ms: u64,
            #[serde(default)]
            snaps: u64,
            #[serde(default, rename = "snapMs")]
            snap_ms: u64,
            #[serde(default)]
            trimmed: bool,
            #[serde(default, borrow)]
            trace: Cow<'a, str>,
            #[serde(default, borrow)]
            windows: Vec<Window<'a>>,
            #[serde(default, borrow)]
            nodes: Option<Vec<Node<'a>>>,
        }
        #[derive(serde::Deserialize)]
        struct Window<'a> {
            #[serde(default, borrow)]
            pkg: Cow<'a, str>,
            #[serde(default, rename = "type")]
            kind: i64,
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
        let has_screen = r.nodes.is_some();
        // One package string per window, shared by its nodes; one class string per kind.
        let packages: Vec<std::sync::Arc<str>> = r.windows.iter().map(|w| std::sync::Arc::from(&*w.pkg)).collect();
        let mut classes: Vec<std::sync::Arc<str>> = Vec::new();
        let mut class = |k: &str| -> std::sync::Arc<str> {
            match classes.iter().find(|c| &***c == k) {
                Some(c) => c.clone(),
                None => {
                    let c: std::sync::Arc<str> = k.into();
                    classes.push(c.clone());
                    c
                }
            }
        };
        let elements = r
            .nodes
            .unwrap_or_default()
            .into_iter()
            .map(|n| Element {
                depth: n.d,
                window: n.w,
                bounds: Rect { left: n.b[0], top: n.b[1], right: n.b[2], bottom: n.b[3] },
                text: n.t.into_owned(),
                desc: n.c.into_owned(),
                id: n.i.into_owned(),
                class: class(&n.k),
                hint: n.h.map(Cow::into_owned).unwrap_or_default(),
                error: n.e.map(Cow::into_owned).unwrap_or_default(),
                flags: n.f | if r.windows.get(n.w as usize).is_some_and(|w| w.kind == WINDOW_TYPE_IME) { KEYBOARD } else { 0 },
                parent: None,
                package: packages.get(n.w as usize).cloned().unwrap_or_default(),
            })
            .collect();
        let mut tree = ScreenTree::new(elements, width, height);
        tree.seq = r.seq;
        Ok(ScreenReply {
            tree,
            seq: r.seq,
            unchanged: r.unchanged,
            changed: r.changed,
            timed_out: r.timed_out,
            has_screen,
            settle_trace: (r.idle_ms, r.snaps, r.snap_ms),
            trimmed: r.trimmed,
            trace: r.trace.into_owned(),
        })
    }
}
