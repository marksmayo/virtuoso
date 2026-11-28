//! Text drawn on the screen, as a device recognises it in a screenshot (iOS: Vision's
//! VNRecognizeTextRequest, run in the agent): what a text selector falls back to once the
//! accessibility tree has had the whole of the step's wait to show it and has not.
//!
//! - Only a selector of text alone (an `index` too) falls back: an id, a state, a relation or a
//!   size says nothing about pixels.
//! - Each recognised line stands for an element whose text is the line, and the selector's text
//!   matches it as it matches an element's ([`crate::select::Pattern`]): any case, a regex over
//!   the whole line, never a part of it unless the regex says so (`.*Coupon.*`).
//! - A merged label (iOS joins a container's texts with ", " when it is one accessibility
//!   element: "Email, Password, SIGN IN") names where to look: a line drawn inside the frame of
//!   an element one of whose label's parts the selector matches is taken before one elsewhere.
//! - What a selector finds this way has the line's frame, so a tap lands on the text's centre.

use crate::flow::Selector;
use crate::screen::{Element, Rect, ScreenTree, ENABLED, VISIBLE};
use crate::select::{select, Pattern};
use serde_json::Value;

/// A line of text recognised on the screen, its frame in screen points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    pub bounds: Rect,
}

/// The kind a recognised line has in the screen a lookup gets back.
pub const CLASS: &str = "RecognizedText";

/// Whether text recognition is on, by VIRTUOSO_TEXT_RECOGNITION's value: on unless it is `0`,
/// `off`, `false` or `no` (any case).
pub fn enabled(var: Option<&str>) -> bool {
    !var.is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "off" | "false" | "no"))
}

/// Whether `sel` can be looked for in recognised text: it asks for a text and nothing else
/// recognition cannot see (an `index` it can: the lines are in order).
pub fn applies(sel: &Selector) -> bool {
    let bare = Selector { text: sel.text.clone(), index: sel.index.clone(), ..Default::default() };
    sel.text.is_some() && *sel == bare
}

/// The lines of an agent's `recognize` reply: `{"lines": [{"t": text, "b": [l, t, r, b]}]}`. A
/// line with no text or no four-number frame is left out.
pub fn parse(reply: &Value) -> Vec<Line> {
    let empty = vec![];
    reply["lines"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .filter_map(|l| {
            let text = l["t"].as_str().filter(|t| !t.trim().is_empty())?.to_string();
            let b = l["b"].as_array().filter(|b| b.len() == 4)?;
            let n = |i: usize| b[i].as_f64().map(|v| v.round() as i32);
            Some(Line { text, bounds: Rect { left: n(0)?, top: n(1)?, right: n(2)?, bottom: n(3)? } })
        })
        .collect()
}

/// The frames of the elements on `screen` whose label has a part `sel`'s text matches wholly:
/// "SIGN IN" in "Email, Password, SIGN IN".
pub fn merged_frames(screen: &ScreenTree, sel: &Selector) -> Vec<Rect> {
    let Some(text) = sel.text.as_deref() else { return vec![] };
    let p = Pattern::cached(text);
    (0..screen.elements.len())
        .filter(|&i| screen.matchable(i))
        .map(|i| &screen.elements[i])
        .filter(|e| [&e.text, &e.desc, &e.hint].iter().any(|v| v.contains(',') && v.split(',').any(|part| p.matches(part.trim()))))
        .map(|e| e.bounds)
        .collect()
}

/// The screen of the recognised lines: the screen's frame, then a line each.
fn lines_tree(lines: &[Line], width: i32, height: i32) -> ScreenTree {
    let class: std::sync::Arc<str> = CLASS.into();
    let root = Element { bounds: Rect { left: 0, top: 0, right: width, bottom: height }, flags: VISIBLE | ENABLED, ..Default::default() };
    let els = std::iter::once(root).chain(lines.iter().map(|l| Element { depth: 1, text: l.text.clone(), bounds: l.bounds, class: class.clone(), flags: VISIBLE | ENABLED, ..Default::default() }));
    ScreenTree::new(els.collect(), width, height)
}

/// The line `sel` finds among `lines`, as a screen of the screen's frame and that line alone
/// (element 1): what the lookup hands back. A line inside a merged label's frame on `screen`
/// (the tree that did not have the text) comes first. None when no line matches.
pub fn find(lines: &[Line], screen: &ScreenTree, sel: &Selector) -> Result<Option<ScreenTree>, String> {
    let all = lines_tree(lines, screen.width, screen.height);
    let found = select(&all, sel)?;
    let merged = merged_frames(screen, sel);
    let inside = |&&i: &&usize| {
        let (x, y) = all.elements[i].bounds.center();
        merged.iter().any(|m| m.contains(x, y))
    };
    let line = match found.iter().find(inside).or(found.first()) {
        Some(&i) => Line { text: all.elements[i].text.clone(), bounds: all.elements[i].bounds },
        // An arrow merged into a label is drawn there; Vision's text model reads it as ASCII.
        None => match sel.text.as_deref().map(ascii_readings) {
            Some(readings) if !readings.is_empty() => {
                let read = |l: &&Line| readings.contains(&l.text.trim()) && merged.iter().any(|m| m.contains(l.bounds.center().0, l.bounds.center().1));
                match lines.iter().find(read) {
                    Some(l) => l.clone(),
                    None => return Ok(None),
                }
            }
            _ => return Ok(None),
        },
    };
    Ok(Some(lines_tree(&[line], screen.width, screen.height)))
}

/// What Vision's text model reads a drawn symbol as: an arrow comes back as ASCII. Only the
/// symbols it is known to do this to; none for any other text.
fn ascii_readings(symbol: &str) -> &'static [&'static str] {
    match symbol.trim() {
        "→" => &["->", ">", "=>"],
        "←" => &["<-", "<", "<="],
        _ => &[],
    }
}

/// The recognised texts, for a failed lookup's error: each quoted, in order, the first 40.
pub fn describe(lines: &[Line]) -> String {
    if lines.is_empty() {
        return "no text".into();
    }
    lines.iter().take(40).map(|l| format!("{:?}", l.text)).collect::<Vec<_>>().join(" | ")
}
