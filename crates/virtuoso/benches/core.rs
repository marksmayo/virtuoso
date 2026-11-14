//! Host-side hot paths: what virtuoso does with the CPU between device round trips (parsing
//! flows, reading a screen reply, finding elements, `${...}` expansion, screenshot pixels).
//!
//! Run with `cargo bench -p virtuoso --bench core`, or under CodSpeed with
//! `cargo codspeed build -p virtuoso && cargo codspeed run -p virtuoso`.

use divan::{black_box, Bencher};
use virtuoso::flow::{parse_flow, Selector};
use virtuoso::image_util::{self, Rgba};
use virtuoso::js::Js;
use virtuoso::screen::{ScreenReply, ScreenTree, CLICKABLE, ENABLED, VISIBLE};
use virtuoso::select::select;

fn main() {
    divan::main();
}

// ---------------------------------------------------------------------------------------------
// Inputs

/// A flow as a real suite writes one: launch, taps, input, waits, conditions, scripts.
const FLOW: &str = r#"# A sign-in flow with the commands most flows use.
appId: com.example.app
name: Sign in
env:
  USER: alice@example.com
  PASSWORD: secret
tags:
  - smoke
  - auth
---
- launchApp:
    clearState: true
- assertVisible: "Welcome"
- tapOn: "Sign in"
- tapOn:
    id: "email_field"
- inputText: ${USER}
- tapOn:
    id: "password_field"
- inputText: ${PASSWORD}
- hideKeyboard
- tapOn:
    text: "Continue"
    below: "Password"
- extendedWaitUntil:
    visible: "Home"
    timeout: 10000
- assertVisible:
    text: "Item \\d+"
    index: 2
- scrollUntilVisible:
    element:
      text: "Settings"
    direction: DOWN
- runFlow:
    when:
      visible: "Accept cookies"
    commands:
      - tapOn: "Accept"
- swipe:
    direction: LEFT
- evalScript: ${output.count = 3}
- assertTrue: ${output.count == 3}
- back
- assertNotVisible: "Error"
- repeat:
    times: 3
    commands:
      - tapOn: "Next"
      - assertVisible: "Page.*"
- takeScreenshot: signed-in
- stopApp
"#;

/// An agent `screen` reply with `rows` list rows (each a clickable container, a title, a
/// subtitle and an icon) under a toolbar: ~4 nodes a row, as a settings-like screen has.
fn screen_reply(rows: usize) -> String {
    let mut nodes = vec![
        r#"{"d":0,"w":0,"f":34,"k":"android.widget.FrameLayout","b":[0,0,1080,2400]}"#.to_string(),
        r#"{"d":1,"w":0,"f":34,"k":"android.widget.LinearLayout","i":"com.example.app:id/toolbar","b":[0,0,1080,200]}"#.to_string(),
        r#"{"d":2,"w":0,"f":35,"t":"Settings","k":"android.widget.TextView","b":[40,60,600,140]}"#.to_string(),
        r#"{"d":2,"w":0,"f":35,"c":"Search","k":"android.widget.ImageButton","i":"com.example.app:id/search","b":[960,60,1040,140]}"#.to_string(),
        r#"{"d":1,"w":0,"f":42,"k":"androidx.recyclerview.widget.RecyclerView","i":"com.example.app:id/list","b":[0,200,1080,2400]}"#.to_string(),
    ];
    for r in 0..rows {
        let top = 200 + r as i32 * 180;
        let bottom = top + 180;
        nodes.push(format!(
            r#"{{"d":2,"w":0,"f":35,"k":"android.widget.LinearLayout","b":[0,{top},1080,{bottom}]}}"#
        ));
        nodes.push(format!(
            r#"{{"d":3,"w":0,"f":34,"c":"Icon {r}","k":"android.widget.ImageView","i":"com.example.app:id/icon","b":[40,{},140,{}]}}"#,
            top + 40,
            top + 140
        ));
        nodes.push(format!(
            r#"{{"d":3,"w":0,"f":34,"t":"Item {r}","k":"android.widget.TextView","i":"com.example.app:id/title","b":[180,{},1040,{}]}}"#,
            top + 30,
            top + 90
        ));
        nodes.push(format!(
            r#"{{"d":3,"w":0,"f":34,"t":"Details about item number {r}","k":"android.widget.TextView","i":"com.example.app:id/summary","b":[180,{},1040,{}]}}"#,
            top + 100,
            top + 150
        ));
    }
    format!(
        r#"{{"ok":true,"seq":42,"windows":[{{"pkg":"com.example.app","type":1}}],"nodes":[{}]}}"#,
        nodes.join(",")
    )
}

fn screen(rows: usize) -> ScreenTree {
    ScreenReply::parse(screen_reply(rows).as_bytes(), 1080, 2400).unwrap().tree
}

fn text(t: &str) -> Selector {
    Selector { text: Some(t.into()), ..Default::default() }
}

/// A screenshot-like image: gradients with some flat areas, so it compresses as a real one.
fn image(width: u32, height: u32) -> Rgba {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let flat = (y / 64) % 2 == 0;
            let (r, g, b) = if flat { (240, 240, 245) } else { ((x * 255 / width) as u8, (y * 255 / height) as u8, ((x ^ y) & 0xff) as u8) };
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }
    Rgba { width, height, pixels }
}

/// The image as a device writes a screenshot: deflate-compressed, filtered rows.
fn compressed_png(img: &Rgba) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, img.width, img.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&img.pixels).unwrap();
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Flows

#[divan::bench]
fn parse_flow_sign_in() {
    black_box(parse_flow(black_box(FLOW), None).unwrap());
}

#[divan::bench]
fn parse_env_file(bencher: Bencher) {
    let text: String = (0..50).map(|i| format!("# setting {i}\nKEY_{i}=\"value number {i}\"\nexport OTHER_{i}=plain_{i}\n")).collect();
    bencher.bench(|| black_box(virtuoso::envfile::parse_env_file(black_box(&text)).unwrap()));
}

// ---------------------------------------------------------------------------------------------
// Screen reads

#[divan::bench(args = [10, 60, 250])]
fn parse_screen_reply(bencher: Bencher, rows: usize) {
    let body = screen_reply(rows);
    bencher.bench(|| black_box(ScreenReply::parse(black_box(body.as_bytes()), 1080, 2400).unwrap()));
}

#[divan::bench(args = [60])]
fn screen_from_agent_value(bencher: Bencher, rows: usize) {
    let value: serde_json::Value = serde_json::from_str(&screen_reply(rows)).unwrap();
    bencher.bench(|| black_box(ScreenTree::from_agent(black_box(&value), 1080, 2400)));
}

#[divan::bench(args = [60])]
fn screen_fingerprint(bencher: Bencher, rows: usize) {
    let s = screen(rows);
    bencher.bench(|| black_box(black_box(&s).fingerprint()));
}

// ---------------------------------------------------------------------------------------------
// Selectors

#[divan::bench(args = [60, 250])]
fn select_text(bencher: Bencher, rows: usize) {
    let s = screen(rows);
    let sel = text("Item 9");
    assert!(!select(&s, &sel).unwrap().is_empty(), "the selector finds its element");
    bencher.bench(|| black_box(select(black_box(&s), black_box(&sel)).unwrap()));
}

#[divan::bench(args = [60])]
fn select_regex(bencher: Bencher, rows: usize) {
    let s = screen(rows);
    let sel = text("Details about item number \\d*7");
    assert!(!select(&s, &sel).unwrap().is_empty(), "the selector finds its element");
    bencher.bench(|| black_box(select(black_box(&s), black_box(&sel)).unwrap()));
}

#[divan::bench(args = [60])]
fn select_id_with_index(bencher: Bencher, rows: usize) {
    let s = screen(rows);
    let sel = Selector { id: Some("com.example.app:id/title".into()), index: Some("-1".into()), ..Default::default() };
    assert!(!select(&s, &sel).unwrap().is_empty(), "the selector finds its element");
    bencher.bench(|| black_box(select(black_box(&s), black_box(&sel)).unwrap()));
}

#[divan::bench(args = [60])]
fn select_relative_below(bencher: Bencher, rows: usize) {
    let s = screen(rows);
    let sel = Selector { text: Some("Details.*".into()), below: Some(Box::new(text("Item 3"))), ..Default::default() };
    assert!(!select(&s, &sel).unwrap().is_empty(), "the selector finds its element");
    bencher.bench(|| black_box(select(black_box(&s), black_box(&sel)).unwrap()));
}

#[divan::bench(args = [60])]
fn select_child_of(bencher: Bencher, rows: usize) {
    let s = screen(rows);
    let sel = Selector {
        id: Some("com.example.app:id/summary".into()),
        child_of: Some(Box::new(Selector { contains_child: Some(Box::new(text("Item 5"))), ..Default::default() })),
        ..Default::default()
    };
    assert!(!select(&s, &sel).unwrap().is_empty(), "the selector finds its element");
    bencher.bench(|| black_box(select(black_box(&s), black_box(&sel)).unwrap()));
}

#[divan::bench(args = [60])]
fn element_at_and_clickable(bencher: Bencher, rows: usize) {
    let s = screen(rows);
    bencher.bench(|| {
        let s = black_box(&s);
        let hit = s.element_at(500, 200 + 5 * 180 + 60);
        black_box(hit.map(|i| s.elements[i].has(CLICKABLE | ENABLED | VISIBLE)))
    });
}

// ---------------------------------------------------------------------------------------------
// `${...}` expressions

#[divan::bench]
fn js_expand_variable(bencher: Bencher) {
    let js = Js::new("android");
    js.set_var("USER", "alice@example.com");
    bencher.bench_local(|| black_box(js.expand(black_box("Signed in as ${USER}")).unwrap()));
}

#[divan::bench]
fn js_expand_expression(bencher: Bencher) {
    let js = Js::new("android");
    js.set_var("N", "4");
    // Start the engine once, outside the measurement: the cost here is the evaluation.
    js.expand("${1}").unwrap();
    bencher.bench_local(|| black_box(js.expand(black_box("${parseInt(N) * 2} items")).unwrap()));
}

// ---------------------------------------------------------------------------------------------
// Screenshot pixels (cropOn, assertScreenshot)

#[divan::bench]
fn png_decode_compressed(bencher: Bencher) {
    let png = compressed_png(&image(360, 640));
    bencher.bench(|| black_box(image_util::decode(black_box(&png)).unwrap()));
}

#[divan::bench]
fn png_encode(bencher: Bencher) {
    let img = image(360, 640);
    bencher.bench(|| black_box(image_util::encode(black_box(&img))));
}

#[divan::bench]
fn png_similarity(bencher: Bencher) {
    let a = compressed_png(&image(360, 640));
    let mut other = image(360, 640);
    for p in other.pixels.chunks_mut(4).step_by(7) {
        p[0] = p[0].wrapping_add(40);
    }
    let b = compressed_png(&other);
    bencher.bench(|| black_box(image_util::similarity_png(black_box(&a), black_box(&b)).unwrap()));
}

#[divan::bench]
fn png_crop(bencher: Bencher) {
    let png = compressed_png(&image(360, 640));
    bencher.bench(|| black_box(image_util::crop_png(black_box(&png), 40, 100, 320, 400).unwrap()));
}
