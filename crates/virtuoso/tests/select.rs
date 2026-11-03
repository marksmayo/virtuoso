//! Finding elements on a screen with Maestro selectors.

use virtuoso::flow::Selector;
use virtuoso::screen::{Element, Rect, ScreenTree, CHECKED, CLICKABLE, ENABLED, FOCUSED, SELECTED, VISIBLE};
use virtuoso::select::select;

/// A node at `depth` (parents are the closest shallower node before it), visible and enabled.
fn n(depth: u16, text: &str, b: [i32; 4]) -> Element {
    Element {
        depth,
        text: text.into(),
        bounds: Rect { left: b[0], top: b[1], right: b[2], bottom: b[3] },
        flags: VISIBLE | ENABLED,
        ..Default::default()
    }
}

fn screen(nodes: Vec<Element>) -> ScreenTree {
    ScreenTree::new(nodes, 1080, 2400)
}

fn texts(s: &ScreenTree, sel: &Selector) -> Vec<String> {
    select(s, sel).unwrap().iter().map(|&i| s.elements[i].text.clone()).collect()
}

fn text(t: &str) -> Selector {
    Selector { text: Some(t.into()), ..Default::default() }
}

#[test]
fn text_matches_the_whole_value_ignoring_case() {
    let s = screen(vec![n(0, "", [0, 0, 1080, 2400]), n(1, "Sign in", [0, 0, 100, 50]), n(1, "Sign in later", [0, 60, 100, 110])]);
    assert_eq!(texts(&s, &text("sign in")), vec!["Sign in"]);
}

#[test]
fn text_is_a_regex() {
    let s = screen(vec![n(0, "Item 12", [0, 0, 100, 50]), n(0, "Item x", [0, 60, 100, 110])]);
    assert_eq!(texts(&s, &text("Item \\d+")), vec!["Item 12"]);
}

#[test]
fn an_invalid_regex_matches_as_plain_text() {
    let s = screen(vec![n(0, "Price (USD", [0, 0, 100, 50])]);
    assert_eq!(texts(&s, &text("Price (USD")), vec!["Price (USD"]);
}

#[test]
fn regex_special_characters_in_plain_text_still_match_literally() {
    let s = screen(vec![n(0, "What's new?", [0, 0, 100, 50])]);
    assert_eq!(texts(&s, &text("What's new?")), vec!["What's new?"]);
}

#[test]
fn text_also_matches_content_description_and_hint() {
    let mut a = n(0, "", [0, 0, 100, 50]);
    a.desc = "Close".into();
    let mut b = n(0, "", [0, 60, 100, 110]);
    b.hint = "Email".into();
    let s = screen(vec![a, b]);
    assert_eq!(select(&s, &text("Close")).unwrap(), vec![0]);
    assert_eq!(select(&s, &text("Email")).unwrap(), vec![1]);
}

#[test]
fn id_matches_the_full_resource_id_or_the_part_after_the_slash() {
    let mut a = n(0, "", [0, 0, 100, 50]);
    a.id = "com.app:id/login_button".into();
    let s = screen(vec![a]);
    let by = |id: &str| select(&s, &Selector { id: Some(id.into()), ..Default::default() }).unwrap();
    assert_eq!(by("login_button"), vec![0]);
    assert_eq!(by("com.app:id/login_button"), vec![0]);
    assert_eq!(by("login.*"), vec![0]);
    assert!(by("login").is_empty());
}

#[test]
fn invisible_and_off_screen_nodes_do_not_match() {
    let mut hidden = n(0, "Hidden", [0, 0, 100, 50]);
    hidden.flags = ENABLED;
    let s = screen(vec![hidden, n(0, "Off", [0, 3000, 100, 3100]), n(0, "Edge", [-95, 0, 5, 100])]);
    assert!(texts(&s, &text("Hidden")).is_empty());
    assert!(texts(&s, &text("Off")).is_empty());
    assert!(texts(&s, &text("Edge")).is_empty(), "a node 5% on screen is dropped");
}

#[test]
fn state_fields_filter_on_flags() {
    let mut on = n(0, "Wifi", [0, 0, 100, 50]);
    on.flags |= CHECKED | FOCUSED | SELECTED;
    let mut off = n(0, "Wifi", [0, 60, 100, 110]);
    off.flags = VISIBLE;
    let s = screen(vec![on, off]);
    let with = |f: fn(&mut Selector)| {
        let mut sel = text("Wifi");
        f(&mut sel);
        select(&s, &sel).unwrap()
    };
    assert_eq!(with(|s| s.checked = Some(true)), vec![0]);
    assert_eq!(with(|s| s.checked = Some(false)), vec![1]);
    assert_eq!(with(|s| s.enabled = Some(false)), vec![1]);
    assert_eq!(with(|s| s.focused = Some(true)), vec![0]);
    assert_eq!(with(|s| s.selected = Some(true)), vec![0]);
}

#[test]
fn index_picks_the_nth_match_in_screen_order() {
    let s = screen(vec![n(0, "Row", [0, 0, 100, 50]), n(0, "Row", [0, 60, 100, 110]), n(0, "Row", [0, 120, 100, 170])]);
    let mut sel = text("Row");
    sel.index = Some("1".into());
    assert_eq!(select(&s, &sel).unwrap(), vec![1]);
    sel.index = Some("-1".into());
    assert_eq!(select(&s, &sel).unwrap(), vec![2], "a negative index counts from the end");
}

#[test]
fn below_and_above_keep_the_screen_order_not_the_closest_first() {
    // As in Maestro, whose relative filters are intersected with the text filter's matches
    // in that filter's order: `{text: Item, above: {id: result}}` is the first Item.
    let s = screen(vec![
        n(0, "Name", [0, 0, 500, 50]),
        n(0, "Field", [0, 300, 500, 350]),
        n(0, "Field", [0, 60, 500, 110]),
        n(0, "Footer", [0, 900, 500, 950]),
    ]);
    let mut sel = text("Field");
    sel.below = Some(Box::new(text("Name")));
    assert_eq!(select(&s, &sel).unwrap(), vec![1, 2]);
    let mut sel = text("Field");
    sel.above = Some(Box::new(text("Footer")));
    assert_eq!(select(&s, &sel).unwrap(), vec![1, 2]);
}

#[test]
fn left_of_and_right_of_compare_horizontal_positions() {
    let s = screen(vec![n(0, "Label", [0, 0, 100, 50]), n(0, "X", [200, 0, 300, 50]), n(0, "X", [400, 0, 500, 50])]);
    let mut sel = text("X");
    sel.right_of = Some(Box::new(text("Label")));
    assert_eq!(select(&s, &sel).unwrap(), vec![1, 2]);
    let mut sel = text("Label");
    sel.left_of = Some(Box::new(text("X")));
    assert_eq!(select(&s, &sel).unwrap(), vec![0]);
}

#[test]
fn child_of_and_contains_child_follow_the_hierarchy() {
    let mut card_a = n(0, "", [0, 0, 500, 200]);
    card_a.id = "card".into();
    let mut card_b = n(0, "", [0, 300, 500, 500]);
    card_b.id = "card".into();
    let s = screen(vec![
        card_a,
        n(1, "Alice", [0, 0, 200, 50]),
        n(1, "Delete", [300, 0, 500, 50]),
        card_b,
        n(1, "Bob", [0, 300, 200, 350]),
        n(1, "Delete", [300, 300, 500, 350]),
    ]);
    let mut card_with_bob = Selector { id: Some("card".into()), ..Default::default() };
    card_with_bob.contains_child = Some(Box::new(text("Bob")));
    assert_eq!(select(&s, &card_with_bob).unwrap(), vec![3]);
    let mut delete = text("Delete");
    delete.child_of = Some(Box::new(card_with_bob));
    assert_eq!(select(&s, &delete).unwrap(), vec![5]);
}

#[test]
fn contains_descendants_needs_all_of_them() {
    let s = screen(vec![
        n(0, "", [0, 0, 500, 500]),
        n(1, "", [0, 0, 500, 250]),
        n(2, "A", [0, 0, 100, 50]),
        n(2, "B", [0, 60, 100, 110]),
        n(1, "", [0, 260, 500, 500]),
        n(2, "A", [0, 260, 100, 300]),
    ]);
    let sel = Selector { contains_descendants: vec![text("A"), text("B")], ..Default::default() };
    assert_eq!(select(&s, &sel).unwrap(), vec![0, 1]);
}

#[test]
fn width_and_height_match_within_tolerance() {
    let s = screen(vec![n(0, "", [0, 0, 100, 40]), n(0, "", [0, 50, 120, 90])]);
    let sel = Selector { width: Some(102), height: Some(40), tolerance: Some(3), ..Default::default() };
    assert_eq!(select(&s, &sel).unwrap(), vec![0]);
}

#[test]
fn of_nested_matches_with_the_same_text_the_deepest_wins() {
    let mut parent = n(0, "", [0, 0, 500, 100]);
    parent.desc = "Buy".into();
    parent.flags |= CLICKABLE;
    let s = screen(vec![parent, n(1, "Buy", [10, 10, 490, 90])]);
    assert_eq!(select(&s, &text("Buy")).unwrap(), vec![1]);
}

#[test]
fn screen_tree_reads_the_agent_reply() {
    let reply = serde_json::json!({
        "windows": [{"pkg": "com.app", "active": true, "type": 1, "layer": 1, "b": [0,0,1080,2400]}],
        "nodes": [
            {"d": 0, "w": 0, "b": [0,0,1080,2400], "f": 34, "t": "", "c": "", "i": "", "k": "android.widget.FrameLayout"},
            {"d": 1, "w": 0, "b": [0,0,100,50], "f": 35, "t": "OK", "c": "", "i": "com.app:id/ok", "k": "android.widget.Button", "h": "hint"}
        ]
    });
    let s = ScreenTree::from_agent(&reply, 1080, 2400);
    assert_eq!(s.elements.len(), 2);
    assert_eq!(s.elements[1].parent, Some(0));
    assert_eq!(s.elements[1].hint, "hint");
    assert!(s.elements[1].clickable());
    assert_eq!(s.elements[1].package, "com.app");
}

#[test]
fn text_patterns_take_lookarounds_and_backreferences() {
    use virtuoso::select::Pattern;
    assert!(Pattern::new("(?=.*Sign).*in").matches("Sign in"));
    assert!(!Pattern::new("(?!Sign).*in").matches("Sign in"));
    assert!(Pattern::new(r"(\w)\1.*").matches("oops"));
    assert!(!Pattern::new(r"(\w)\1.*").matches("Good"));
}

#[test]
fn plain_text_patterns_match_without_compiling_a_regex() {
    use virtuoso::select::Pattern;
    let p = Pattern::new("Network & internet");
    assert!(!p.compiled());
    assert!(p.matches("network & INTERNET"));
    assert!(!p.matches("Network & internet settings"));
    let r = Pattern::new("Net.*");
    assert!(r.compiled());
    assert!(r.matches("network & internet"));
}

#[test]
fn a_screen_reply_parses_straight_from_its_bytes() {
    use virtuoso::screen::ScreenReply;
    let body = r#"{"ok":true,"seq":42,"windows":[{"pkg":"com.app","type":1,"active":true,"b":[0,0,1080,2400]},{"pkg":"com.android.systemui","type":3,"active":false,"b":[0,0,1080,80]}],
        "nodes":[{"d":0,"w":0,"b":[0,0,1080,2400],"f":34,"k":"android.widget.FrameLayout"},
                 {"d":1,"w":0,"b":[0,0,100,50],"f":35,"t":"Say \"hi\"\né 😀","i":"com.app:id/ok","k":"android.widget.Button","h":"hint","e":"bad"},
                 {"d":0,"w":1,"b":[0,0,1080,80],"f":32,"c":"Battery"}],"us":812}"#;
    let reply = ScreenReply::parse(body.as_bytes(), 1080, 2400).unwrap();
    let expected = ScreenTree::from_agent(&serde_json::from_str(body).unwrap(), 1080, 2400);
    assert_eq!(reply.seq, Some(42));
    assert!(!reply.unchanged);
    assert_eq!(reply.tree.elements, expected.elements);
    assert_eq!(reply.tree.elements[1].text, "Say \"hi\"\n\u{e9} \u{1f600}");
    assert_eq!(reply.tree.elements[2].package, "com.android.systemui");

    let same = ScreenReply::parse(br#"{"ok":true,"unchanged":true,"seq":42,"us":3}"#, 1080, 2400).unwrap();
    assert!(same.unchanged);
    assert!(same.tree.elements.is_empty());
    assert!(ScreenReply::parse(br#"{"ok":false,"error":"boom"}"#, 1080, 2400).unwrap_err().to_string().contains("boom"));
}

#[test]
fn text_patterns_keep_word_boundaries_unicode_case_and_classes() {
    // The regex engine is built with only the features selectors use: these must still work
    // (and a word boundary must not panic, as it does without unicode-word-boundary).
    use virtuoso::select::Pattern;
    assert!(Pattern::new(r"\bWi-Fi\b.*").matches("Wi-Fi settings"));
    assert!(!Pattern::new(r".*\bFi\b").matches("WiFi"));
    assert!(Pattern::new("(?i)ÄRGER").matches("ärger"));
    assert!(Pattern::new("CAFÉ.*").matches("café au lait"), "case-insensitive by default, accents too");
    assert!(Pattern::new(r"\w+ \d+").matches("Größe 42"));
    assert!(Pattern::new(r"\s*Total:\s+\d+").matches(" Total:  7"));
    assert!(Pattern::new(r"(?<=Sign )in").matches("Sign in") || Pattern::new(r".*(?<=Sign )in").matches("Sign in"));
    assert!(Pattern::new(r"(a)\1b").matches("aab"));
    assert!(Pattern::new("[0-9]{3}-[0-9]{4}").matches("555-1234"));
}

#[test]
fn a_pattern_the_engine_cannot_build_still_matches_its_text_literally() {
    // \p{...} classes are left out of the engine: such a pattern compares as plain text.
    use virtuoso::select::Pattern;
    let p = Pattern::new(r"\p{Greek}+");
    assert!(p.matches(r"\p{Greek}+"));
}

#[test]
fn bounds_print_as_android_writes_them() {
    let r = virtuoso::screen::Rect { left: 0, top: 10, right: 100, bottom: 50 };
    assert_eq!(r.to_string(), "[0,10][100,50]");
}

fn spinner(depth: u16, b: [i32; 4]) -> Element {
    Element { class: "android.widget.ProgressBar".into(), ..n(depth, "", b) }
}

#[test]
fn a_see_through_overlay_holding_a_spinner_covers_the_keys_under_it() {
    use virtuoso::select::loading_cover;
    let s = screen(vec![
        n(0, "", [0, 0, 1080, 2400]),
        n(1, "1", [60, 650, 360, 780]),
        n(1, "", [0, 136, 1080, 2400]),
        spinner(2, [500, 1100, 580, 1180]),
    ]);
    assert_eq!(loading_cover(&s, 1), Some(2), "the overlay over the key's centre, with a spinner in it");
    // The spinner's own branch is not over the overlay itself.
    assert_eq!(loading_cover(&s, 3), None);
}

#[test]
fn a_spinner_inside_the_elements_own_container_or_elsewhere_is_no_cover() {
    use virtuoso::select::loading_cover;
    // A list with a spinner at its end: the rows are inside the list, so it covers nothing.
    let s = screen(vec![
        n(0, "", [0, 0, 1080, 2400]),
        n(1, "", [0, 136, 1080, 2400]),
        n(2, "Row", [0, 200, 1080, 300]),
        spinner(2, [500, 2200, 580, 2280]),
    ]);
    assert_eq!(loading_cover(&s, 3), None);
    // A spinner away from the element, in a branch of its own.
    let s = screen(vec![n(0, "", [0, 0, 1080, 2400]), n(1, "Save", [0, 200, 1080, 300]), spinner(1, [500, 2200, 580, 2280])]);
    assert_eq!(loading_cover(&s, 2), None);
}

fn list(depth: u16, class: &str, b: [i32; 4]) -> Element {
    Element { class: class.into(), flags: VISIBLE | ENABLED | virtuoso::screen::SCROLLABLE, ..n(depth, "", b) }
}

#[test]
fn the_sideways_list_is_a_row_of_items_or_a_horizontal_view_nearest_the_centre() {
    let s = screen(vec![
        n(0, "", [0, 0, 1080, 2400]),
        // A vertical list across the centre: its items are a column.
        list(1, "androidx.recyclerview.widget.RecyclerView", [0, 1000, 1080, 2400]),
        n(2, "Row 1", [0, 1000, 1080, 1100]),
        n(2, "Row 2", [0, 1100, 1080, 1200]),
        // A row of cards above it.
        list(1, "android.widget.HorizontalScrollView", [0, 400, 1080, 700]),
        n(2, "Card 1", [0, 400, 500, 700]),
        n(2, "Card 2", [520, 400, 1020, 700]),
    ]);
    assert_eq!(s.sideways_list(), Some(4));
    let s = screen(vec![
        n(0, "", [0, 0, 1080, 2400]),
        list(1, "androidx.recyclerview.widget.RecyclerView", [0, 300, 1080, 600]),
        n(2, "Card 1", [0, 300, 500, 600]),
        n(2, "Card 2", [520, 300, 1020, 600]),
    ]);
    assert_eq!(s.sideways_list(), Some(1), "a RecyclerView whose items sit side by side");
    let s = screen(vec![n(0, "", [0, 0, 1080, 2400]), list(1, "android.widget.ScrollView", [0, 0, 1080, 2400])]);
    assert_eq!(s.sideways_list(), None);
}
