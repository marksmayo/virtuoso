//! Text drawn on the screen, as a device recognises it in a screenshot. (Red: stubs.)

use crate::flow::Selector;
use crate::screen::{Rect, ScreenTree};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    pub bounds: Rect,
}

pub const CLASS: &str = "RecognizedText";

pub fn enabled(_var: Option<&str>) -> bool {
    true
}

pub fn applies(_sel: &Selector) -> bool {
    false
}

pub fn parse(_reply: &Value) -> Vec<Line> {
    vec![]
}

pub fn merged_frames(_screen: &ScreenTree, _sel: &Selector) -> Vec<Rect> {
    vec![]
}

pub fn find(_lines: &[Line], _screen: &ScreenTree, _sel: &Selector) -> Result<Option<ScreenTree>, String> {
    Ok(None)
}

pub fn describe(_lines: &[Line]) -> String {
    String::new()
}
