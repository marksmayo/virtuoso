//! Text recognised on the screen: what a text selector falls back to, and how it matches.

use serde_json::json;
use virtuoso::flow::Selector;
use virtuoso::recognize::{applies, describe, enabled, find, merged_frames, parse, Line};
use virtuoso::screen::{Element, Rect, ScreenTree, ENABLED, VISIBLE};

fn line(text: &str, b: [i32; 4]) -> Line {
    Line { text: text.into(), bounds: Rect { left: b[0], top: b[1], right: b[2], bottom: b[3] } }
}

fn text(t: &str) -> Selector {
    Selector { text: Some(t.into()), ..Default::default() }
}

/// A screen of the frame and these labelled elements.
fn tree(labels: &[(&str, [i32; 4])]) -> ScreenTree {
    let root = Element { bounds: Rect { left: 0, top: 0, right: 1000, bottom: 2000 }, flags: VISIBLE | ENABLED, ..Default::default() };
    let els = std::iter::once(root).chain(labels.iter().map(|(t, b)| Element {
        depth: 1,
        text: t.to_string(),
        bounds: Rect { left: b[0], top: b[1], right: b[2], bottom: b[3] },
        flags: VISIBLE | ENABLED,
        ..Default::default()
    }));
    ScreenTree::new(els.collect(), 1000, 2000)
}

#[test]
fn recognition_is_on_unless_the_variable_turns_it_off() {
    assert!(enabled(None));
    assert!(enabled(Some("1")));
    assert!(enabled(Some("")));
    for off in ["0", "off", "OFF", "false", "No", " 0 "] {
        assert!(!enabled(Some(off)), "{off:?} turns it off");
    }
}

#[test]
fn only_a_selector_of_text_alone_falls_back_to_recognition() {
    assert!(applies(&text("Coupon")));
    assert!(applies(&Selector { index: Some("1".into()), ..text("Coupon") }), "the lines are in order: an index works");
    assert!(!applies(&Selector::default()), "no text");
    assert!(!applies(&Selector { id: Some("coupon".into()), ..Default::default() }), "an id is not drawn");
    assert!(!applies(&Selector { id: Some("coupon".into()), ..text("Coupon") }));
    assert!(!applies(&Selector { enabled: Some(true), ..text("Coupon") }), "a state is not drawn");
    assert!(!applies(&Selector { below: Some(Box::new(text("Header"))), ..text("Coupon") }), "nor a relation");
    assert!(!applies(&Selector { width: Some(100), ..text("Coupon") }), "nor a size");
    assert!(!applies(&Selector { traits: vec!["text".into()], ..text("Coupon") }), "nor a trait");
}

#[test]
fn the_agents_reply_gives_each_line_and_its_frame() {
    let lines = parse(&json!({"ok": true, "lines": [
        {"t": "SIGN IN", "b": [10, 20.4, 110.6, 60]},
        {"t": "   ", "b": [0, 0, 1, 1]},
        {"t": "no frame"},
        {"t": "short frame", "b": [1, 2, 3]},
        {"b": [1, 2, 3, 4]},
        {"t": "Sequence complete", "b": [5, 6, 7, 8]}
    ]}));
    assert_eq!(lines, vec![line("SIGN IN", [10, 20, 111, 60]), line("Sequence complete", [5, 6, 7, 8])]);
    assert!(parse(&json!({"ok": true})).is_empty());
}

#[test]
fn recognised_text_matches_as_an_elements_text_does() {
    let drawn = [line("Sequence Complete", [100, 900, 500, 960])];
    let screen = tree(&[]);
    let found = |t: &str| find(&drawn, &screen, &text(t)).unwrap();
    // Any case, the whole line.
    let s = found("sequence complete").expect("any case");
    assert_eq!(s.elements.len(), 2, "the frame and the line");
    assert_eq!(s.elements[1].text, "Sequence Complete");
    assert_eq!(s.elements[1].bounds, Rect { left: 100, top: 900, right: 500, bottom: 960 });
    assert_eq!(&*s.elements[1].class, virtuoso::recognize::CLASS);
    assert!(s.matchable(1), "visible and on screen, as the lookup wants");
    assert!(found("Sequence").is_none(), "a part of a line is no match: the selector's text is a whole text");
    assert!(found("Sequence Complete!").is_none());
    // A regex over the whole line, as on the tree.
    assert!(found(".*Complete").is_some());
    assert!(found("Sequence (Complete|Done)").is_some());
    assert!(found("Sequence \\d+").is_none());
}

#[test]
fn an_index_picks_among_the_recognised_lines_in_order() {
    let drawn = [line("Buy", [0, 100, 100, 140]), line("Buy", [0, 300, 100, 340])];
    let second = find(&drawn, &tree(&[]), &Selector { index: Some("1".into()), ..text("Buy") }).unwrap().unwrap();
    assert_eq!(second.elements[1].bounds.top, 300);
}

#[test]
fn a_part_of_a_merged_label_is_found_where_it_is_drawn_inside_that_element() {
    // "SIGN IN" is only a part of one element's label; it is drawn twice, once above the form.
    let screen = tree(&[("Use tester, Email, Password, SIGN IN", [0, 400, 1000, 1000])]);
    let drawn = [line("SIGN IN", [100, 100, 300, 140]), line("SIGN IN", [400, 800, 600, 840])];
    let s = find(&drawn, &screen, &text("SIGN IN")).unwrap().unwrap();
    assert_eq!(s.elements[1].bounds.center(), (500, 820), "the one inside the merged element");
    // With no merged label, the first.
    let s = find(&drawn, &tree(&[]), &text("SIGN IN")).unwrap().unwrap();
    assert_eq!(s.elements[1].bounds.center(), (200, 120));
    // Drawn only outside it: still found.
    let s = find(&drawn[..1], &screen, &text("SIGN IN")).unwrap().unwrap();
    assert_eq!(s.elements[1].bounds.center(), (200, 120));
}

#[test]
fn a_merged_label_is_one_whose_parts_the_text_matches_wholly() {
    let screen = tree(&[
        ("Use tester, Email, Password, SIGN IN", [0, 400, 1000, 1000]),
        ("SIGN IN now, later", [0, 0, 10, 10]),
        ("Email Password SIGN IN", [0, 0, 20, 20]),
        // Off the screen: not one.
        ("Other, SIGN IN", [0, 5000, 100, 5100]),
    ]);
    assert_eq!(merged_frames(&screen, &text("sign in")), vec![Rect { left: 0, top: 400, right: 1000, bottom: 1000 }]);
    assert!(merged_frames(&screen, &Selector::default()).is_empty());
}

#[test]
fn no_line_matching_finds_nothing() {
    assert!(find(&[line("Coupon", [0, 0, 10, 10])], &tree(&[]), &text("Apply")).unwrap().is_none());
    assert!(find(&[], &tree(&[]), &text("Apply")).unwrap().is_none());
}

#[test]
fn a_failed_lookup_lists_what_was_recognised() {
    assert_eq!(describe(&[line("Coupon", [0, 0, 1, 1]), line("TA-BENCH-50", [0, 0, 1, 1])]), "\"Coupon\" | \"TA-BENCH-50\"");
    assert_eq!(describe(&[]), "no text");
}
