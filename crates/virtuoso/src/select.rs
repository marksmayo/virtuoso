//! Selector matching, as Maestro does it:
//!
//! - `text` is a case-insensitive regex over the whole of a node's text, content description or
//!   hint; text that is not a valid regex, or that equals the value literally, matches too.
//!   Of nested matches (a button and its label) the deepest is kept.
//! - `id` is a regex over the whole resource id, or the part after `/`.
//! - Only nodes visible to the user and at least 10% on screen match.
//! - `below`/`above`/`leftOf`/`rightOf` keep nodes on that side of the anchor's first match,
//!   closest first; `childOf` keeps descendants of any anchor match.
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

    type Side = fn(&crate::screen::Rect, &crate::screen::Rect) -> Option<i32>;
    let sides: [(&Option<Box<Selector>>, Side); 4] = [
        (&sel.below, |e, a| (e.center().1 >= a.bottom).then(|| e.top - a.bottom)),
        (&sel.above, |e, a| (e.center().1 <= a.top).then(|| a.top - e.bottom)),
        (&sel.left_of, |e, a| (e.center().0 <= a.left).then(|| a.left - e.right)),
        (&sel.right_of, |e, a| (e.center().0 >= a.right).then(|| e.left - a.right)),
    ];
    for (anchor, side) in sides {
        let Some(anchor) = anchor else { continue };
        let Some(&a) = select(screen, anchor)?.first() else {
            return Ok(vec![]);
        };
        let ab = screen.elements[a].bounds;
        let mut kept: Vec<(i32, usize)> =
            out.iter().filter_map(|&i| side(&screen.elements[i].bounds, &ab).map(|d| (d.max(0), i))).collect();
        kept.sort_by_key(|&(d, i)| (d, i));
        out = kept.into_iter().map(|(_, i)| i).collect();
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
