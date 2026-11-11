//! Selector matching, as Maestro does it:
//!
//! - `text` is a case-insensitive regex over the whole of a node's text, content description or
//!   hint; text that is not a valid regex, or that equals the value literally, matches too.
//!   Of nested matches (a button and its label) the deepest is kept.
//! - `id` is a regex over the whole resource id, or the part after `/`.
//! - Only nodes visible to the user and at least 10% on screen match.
//! - `below`/`above`/`leftOf`/`rightOf` keep nodes on that side of the anchor's first match,
//!   in screen order (Maestro intersects them with the other filters' matches, in those
//!   filters' order: not the closest first); `childOf` keeps descendants of any anchor match.
//! - `index` picks from the final list (negative counts from the end).

use crate::flow::Selector;
use crate::screen::{ScreenTree, CHECKED, ENABLED, FOCUSED, SELECTED};
use fancy_regex::{Regex, RegexBuilder};

/// A pattern matched against whole values. The full regex syntax works, lookarounds and
/// backreferences included (fancy-regex; plain patterns run on the `regex` engine).
pub struct Pattern {
    literal: String,
    regex: Option<Regex>,
}

impl Pattern {
    pub fn new(p: &str) -> Pattern {
        // Plain text needs no regex: building one costs ~0.2 ms and ~340 KB of heap.
        let plain = !p.chars().any(|c| r"\.+*?()|[]{}^$".contains(c));
        let regex = if plain { None } else { RegexBuilder::new(&format!("(?is)^(?:{p})$")).delegate_size_limit(1 << 22).build().ok() };
        Pattern { literal: p.to_lowercase(), regex }
    }

    /// Whether matching runs a regex (else it compares text, ignoring case).
    pub fn compiled(&self) -> bool {
        self.regex.is_some()
    }

    /// The pattern for `p`, compiled once per thread: a lookup polls the screen many times.
    pub fn cached(p: &str) -> std::rc::Rc<Pattern> {
        thread_local! {
            static CACHE: std::cell::RefCell<std::collections::HashMap<String, std::rc::Rc<Pattern>>> = Default::default();
        }
        CACHE.with(|c| {
            let mut c = c.borrow_mut();
            if c.len() > 512 {
                c.clear();
            }
            c.entry(p.to_string()).or_insert_with(|| std::rc::Rc::new(Pattern::new(p))).clone()
        })
    }

    pub fn matches(&self, value: &str) -> bool {
        if value.is_empty() {
            return self.literal.is_empty();
        }
        // A pattern that backtracks too long counts as no match.
        value.to_lowercase() == self.literal || self.regex.as_ref().is_some_and(|r| r.is_match(value).unwrap_or(false))
    }
}

/// Indices of the elements `sel` matches, in the order Maestro would pick them.
pub fn select(screen: &ScreenTree, sel: &Selector) -> Result<Vec<usize>, String> {
    let text = sel.text.as_deref().map(Pattern::cached);
    let id = sel.id.as_deref().map(Pattern::cached);
    let mut out: Vec<usize> = (0..screen.elements.len())
        .filter(|&i| screen.matchable(i))
        .filter(|&i| {
            let e = &screen.elements[i];
            if let Some(p) = &text {
                if ![&e.text, &e.desc, &e.hint].iter().any(|v| p.matches(v)) {
                    return false;
                }
            }
            if let Some(p) = &id {
                let short = e.id.rsplit_once('/').map(|(_, s)| s).unwrap_or(&e.id);
                if e.id.is_empty() || !(p.matches(&e.id) || p.matches(short)) {
                    return false;
                }
            }
            let flag = |want: Option<bool>, f: u32| want.is_none_or(|w| e.has(f) == w);
            if !(flag(sel.enabled, ENABLED) && flag(sel.checked, CHECKED) && flag(sel.focused, FOCUSED) && flag(sel.selected, SELECTED)) {
                return false;
            }
            let tol = sel.tolerance.unwrap_or(0);
            if sel.width.is_some_and(|w| (e.bounds.width() - w).abs() > tol) || sel.height.is_some_and(|h| (e.bounds.height() - h).abs() > tol) {
                return false;
            }
            sel.traits.iter().all(|t| has_trait(e, t))
        })
        .collect();

    if text.is_some() {
        let all = out.clone();
        out.retain(|&i| !all.iter().any(|&j| j != i && screen.is_ancestor(i, j)));
    }

    if let Some(anchor) = &sel.child_of {
        let anchors = select(screen, anchor)?;
        out.retain(|&i| anchors.iter().any(|&a| screen.is_ancestor(a, i)));
    }
    if let Some(child) = &sel.contains_child {
        let kids = select(screen, child)?;
        out.retain(|&i| kids.iter().any(|&k| screen.elements[k].parent == Some(i)));
    }
    for d in &sel.contains_descendants {
        let found = select(screen, d)?;
        out.retain(|&i| found.iter().any(|&k| screen.is_ancestor(i, k)));
    }

    type Side = fn(&crate::screen::Rect, &crate::screen::Rect) -> bool;
    let sides: [(&Option<Box<Selector>>, Side); 4] = [
        (&sel.below, |e, a| e.center().1 >= a.bottom),
        (&sel.above, |e, a| e.center().1 <= a.top),
        (&sel.left_of, |e, a| e.center().0 <= a.left),
        (&sel.right_of, |e, a| e.center().0 >= a.right),
    ];
    for (anchor, side) in sides {
        let Some(anchor) = anchor else { continue };
        let Some(&a) = select(screen, anchor)?.first() else {
            return Ok(vec![]);
        };
        let ab = screen.elements[a].bounds;
        out.retain(|&i| side(&screen.elements[i].bounds, &ab));
    }

    if let Some(index) = &sel.index {
        let n: i64 = index.trim().parse().map_err(|_| format!("index `{index}` is not a number"))?;
        let at = if n < 0 { out.len() as i64 + n } else { n };
        return Ok(usize::try_from(at).ok().and_then(|at| out.get(at).copied()).into_iter().collect());
    }
    Ok(out)
}

fn has_trait(e: &crate::screen::Element, t: &str) -> bool {
    match t {
        "text" => !e.text.is_empty(),
        "long-text" => e.text.chars().count() >= 200,
        "square" => {
            let (w, h) = (e.bounds.width(), e.bounds.height());
            w > 0 && (w - h).abs() * 100 <= w * 3
        }
        _ => false,
    }
}

/// A loading overlay over element `el`, as speedrun finds one: a node over the element's centre,
/// outside it and not one of its containers, that is a spinner or holds one. Apps lay one over
/// a screen while it loads, often see-through: the element shows through and stays in the
/// tree, but the overlay takes the touches. The tree's order says nothing (Android orders
/// siblings by place on screen, not as drawn); the parents are the views' own.
pub fn loading_cover(screen: &ScreenTree, el: usize) -> Option<usize> {
    // Android's spinner, iOS's (UIActivityIndicatorView, SwiftUI's ProgressView).
    let busy = |e: &crate::screen::Element| e.class.ends_with("ProgressBar") || e.class == "ActivityIndicator";
    let (x, y) = screen.elements[el].bounds.center();
    let els = &screen.elements;
    // Asked before every tap: most screens have no spinner at all.
    if !els.iter().any(busy) {
        return None;
    }
    (0..els.len()).find(|&k| {
        let c = &els[k];
        if k == el || screen.is_ancestor(el, k) || !c.bounds.contains(x, y) {
            return false;
        }
        busy(c) || (!screen.is_ancestor(k, el) && (0..els.len()).any(|j| busy(&els[j]) && screen.is_ancestor(k, j)))
    })
}

/// A bar over element `el` that would take a tap at its centre: a sticky header or footer over
/// a list (a section's header is in the list itself). A node over the centre, drawn over the
/// element (after it in pre-order: a section's background listed before its cells, as iOS lists
/// them, is behind them), outside the element and not one of its containers, at least half as
/// wide as the element (a bar, not a badge) and smaller than half the screen (a see-through
/// overlay over everything is not one). Nothing in the element's own row of the list is one
/// (iOS 26's Settings lists a cell's backgrounds after its label, at the cell's bounds).
pub fn bar_cover(screen: &ScreenTree, el: usize) -> Option<usize> {
    let e = screen.elements[el].bounds;
    let (x, y) = e.center();
    let half_screen = screen.screen_rect().area() / 2;
    let row = row_of(screen, el);
    (el + 1..screen.elements.len()).find(|&k| {
        let c = &screen.elements[k];
        k != el
            && c.has(crate::screen::VISIBLE)
            && c.bounds.contains(x, y)
            && c.bounds.width() * 2 >= e.width()
            && c.bounds.area() < half_screen
            && !screen.is_ancestor(k, el)
            && !screen.is_ancestor(el, k)
            && !row.is_some_and(|r| screen.is_ancestor(r, k))
    })
}

/// The list's child that holds element `el` (its row), when `el` is in a list below it.
fn row_of(screen: &ScreenTree, el: usize) -> Option<usize> {
    let list = screen.vertical_list_around(el)?;
    let mut i = el;
    while let Some(p) = screen.elements[i].parent {
        if p == list {
            return (i != el).then_some(i);
        }
        i = p;
    }
    None
}
