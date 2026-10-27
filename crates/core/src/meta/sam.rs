//! SAM 3.1 via /v1/responses (stream:true). OWNER: agent "meta".
//! Output text lines look like `<0f>1<|box;x1=..;y1=..;x2=..;y2=..;w=..;h=..|><|mask;...|>`.
//! SAM finds things by shape, not by text. `include_confidence` is rejected (400).

use std::sync::OnceLock;

use anyhow::{Context, Result};
use futures_util::StreamExt;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::client::MetaClient;
use super::responses::{Content, InputItem, ResponseEvent};

/// Box in pixels of the image sent to SAM (xyxy); img_w/img_h = that image's size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SamBox {
    pub x1: u32,
    pub y1: u32,
    pub x2: u32,
    pub y2: u32,
    pub img_w: u32,
    pub img_h: u32,
}

impl SamBox {
    pub fn center(&self) -> (u32, u32) {
        ((self.x1 + self.x2) / 2, (self.y1 + self.y2) / 2)
    }
    pub fn area(&self) -> u64 {
        u64::from(self.x2.saturating_sub(self.x1)) * u64::from(self.y2.saturating_sub(self.y1))
    }
}

/// Regex over all `<|box;...|>` tokens; mask tokens ignored; malformed boxes skipped.
/// Keys may come in any order; extra keys (e.g. a future `c=`) are ignored. Coordinates are
/// clamped to the image; exact duplicates (same box in later frames) are dropped.
pub fn parse_sam_text(text: &str) -> Vec<SamBox> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"<\|box;([^|<>]*)\|>").expect("valid regex"));
    let mut out: Vec<SamBox> = Vec::new();
    for cap in re.captures_iter(text) {
        let mut kv = [None::<f64>; 6];
        for pair in cap[1].split(';') {
            let Some((k, v)) = pair.split_once('=') else { continue };
            let idx = match k.trim() {
                "x1" => 0,
                "y1" => 1,
                "x2" => 2,
                "y2" => 3,
                "w" => 4,
                "h" => 5,
                _ => continue,
            };
            kv[idx] = v.trim().parse::<f64>().ok().filter(|f| f.is_finite());
        }
        let [Some(x1), Some(y1), Some(x2), Some(y2), Some(w), Some(h)] = kv else { continue };
        if w < 1.0 || h < 1.0 {
            continue;
        }
        let clamp = |v: f64, max: f64| v.round().clamp(0.0, max) as u32;
        let b = SamBox {
            x1: clamp(x1.min(x2), w),
            y1: clamp(y1.min(y2), h),
            x2: clamp(x1.max(x2), w),
            y2: clamp(y1.max(y2), h),
            img_w: w as u32,
            img_h: h as u32,
        };
        if b.x2 <= b.x1 || b.y2 <= b.y1 || out.contains(&b) {
            continue;
        }
        out.push(b);
    }
    out
}

/// Rescale a box SAM reported for an image of `img_w`x`img_h` onto an image of `w`x`h`.
fn rescale(b: SamBox, w: u32, h: u32) -> SamBox {
    if b.img_w == w && b.img_h == h {
        return b;
    }
    let sx = f64::from(w) / f64::from(b.img_w);
    let sy = f64::from(h) / f64::from(b.img_h);
    let s = |v: u32, f: f64, max: u32| ((f64::from(v) * f).round() as u32).min(max);
    SamBox { x1: s(b.x1, sx, w), y1: s(b.y1, sy, h), x2: s(b.x2, sx, w), y2: s(b.y2, sy, h), img_w: w, img_h: h }
}

/// A synonym's SAM call that takes longer than this is left out.
const SYNONYM_WAIT: std::time::Duration = std::time::Duration::from_secs(6);
/// The query's own SAM call that takes longer than this counts as failed (SAM answers in 1-3 s; a stalled stream
/// would otherwise hold the click until the HTTP timeout).
const SAM_WAIT: std::time::Duration = std::time::Duration::from_secs(12);

/// Fixed synonyms per shape word: SAM misses some shapes under one name (an outlined button) and finds them under
/// another. Matched on the whole query (lowercase, trimmed); at most 2 per query.
const SYNONYMS: &[(&str, &[&str])] = &[
    ("button", &["rounded button", "clickable button"]),
    ("radio button", &["small circle"]),
    ("checkbox", &["small square box"]),
    ("text input field", &["input box"]),
    ("link", &["underlined text"]),
    ("icon", &["small icon"]),
];

/// The query plus at most 2 fixed synonyms ("button" -> "rounded button", "clickable button"). Unknown query: itself.
pub fn phrases(sam_query: &str) -> Vec<String> {
    let q = sam_query.trim();
    let mut out = vec![q.to_string()];
    let key = q.to_lowercase();
    if let Some((_, more)) = SYNONYMS.iter().find(|(k, _)| *k == key) {
        out.extend(more.iter().take(2).map(|m| m.to_string()));
    }
    out
}

/// Intersection over union of two boxes of the same image (0..1).
pub fn iou(a: &SamBox, b: &SamBox) -> f64 {
    let ix = a.x2.min(b.x2).saturating_sub(a.x1.max(b.x1));
    let iy = a.y2.min(b.y2).saturating_sub(a.y1.max(b.y1));
    let inter = u64::from(ix) * u64::from(iy);
    let union = a.area() + b.area() - inter;
    if union == 0 { 0.0 } else { inter as f64 / union as f64 }
}

/// Boxes of several phrases in one list, in phrase order: a box that overlaps an earlier one (IoU > 0.5) is the same
/// element and is dropped.
pub fn merge_boxes(lists: Vec<Vec<SamBox>>) -> Vec<SamBox> {
    let mut out: Vec<SamBox> = Vec::new();
    for b in lists.into_iter().flatten() {
        if !out.iter().any(|o| iou(o, &b) > 0.5) {
            out.push(b);
        }
    }
    out
}

impl MetaClient {
    /// `sam_segment` for the query and its synonyms (see `phrases`) at the same time, boxes merged. Fails only
    /// when every phrase failed.
    pub async fn sam_segment_all(&self, png: &[u8], sam_query: &str) -> Result<Vec<SamBox>> {
        let list = phrases(sam_query);
        // A synonym is extra help: a slow one is not waited for (the query itself is).
        let calls = list.iter().take(3).enumerate().map(|(i, q)| async move {
            let wait = if i == 0 { SAM_WAIT } else { SYNONYM_WAIT };
            match tokio::time::timeout(wait, self.sam_segment(png, q)).await {
                Ok(r) => r,
                // A stalled answer stream must not hold the click: the query itself fails (the model's point is
                // used), a synonym is just left out.
                Err(_) if i == 0 => Err(anyhow::anyhow!("SAM did not answer within {} s", wait.as_secs())),
                Err(_) => {
                    tracing::warn!(phrase = %q, "sam synonym too slow, not waited for");
                    Ok(Vec::new())
                }
            }
        });
        let results = futures_util::future::join_all(calls).await;
        let mut lists = Vec::new();
        let mut first_err = None;
        for (q, r) in list.iter().zip(results) {
            match r {
                Ok(b) => {
                    tracing::debug!(phrase = %q, n = b.len(), "sam phrase");
                    lists.push(b);
                }
                Err(e) => {
                    tracing::warn!(phrase = %q, error = %e, "sam phrase failed");
                    first_err.get_or_insert(e);
                }
            }
        }
        match (lists.is_empty(), first_err) {
            (true, Some(e)) => Err(e),
            _ => Ok(merge_boxes(lists)),
        }
    }

    /// Send PNG (or JPEG) + short noun phrase ("submit button", "sheet of paper"), collect all
    /// output_text deltas, parse boxes. No hit -> empty Vec. Boxes are in pixels of `png`
    /// (rescaled if SAM reports a different image size).
    pub async fn sam_segment(&self, png: &[u8], query: &str) -> Result<Vec<SamBox>> {
        let reader = image::ImageReader::new(std::io::Cursor::new(png)).with_guessed_format()?;
        let image = match reader.format() {
            Some(image::ImageFormat::Jpeg) => Content::jpeg(png),
            _ => Content::png(png),
        };
        let (w, h) = reader.into_dimensions().context("SAM input is not a readable image")?;
        let body = json!({
            "model": super::MODEL_SAM,
            "stream": true,
            "input": [InputItem::user(vec![Content::text(query), image])],
        });
        let started = std::time::Instant::now();
        let mut deltas = String::new();
        let mut final_text = None;
        let mut events = std::pin::pin!(self.stream_json(body));
        while let Some(ev) = events.next().await {
            match ev? {
                ResponseEvent::TextDelta(d) => deltas.push_str(&d),
                ResponseEvent::Completed(o) => final_text = Some(o.text),
                ResponseEvent::Call(_) => {}
            }
        }
        let text = if deltas.is_empty() { final_text.unwrap_or_default() } else { deltas };
        let boxes: Vec<SamBox> = parse_sam_text(&text).into_iter().map(|b| rescale(b, w, h)).collect();
        tracing::debug!(n = boxes.len(), ms = started.elapsed().as_millis() as u64, "sam_segment");
        Ok(boxes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real SAM 3.1 output (2026-10-26) for "button" on an 800x500 form, mask shortened.
    const REAL: &str = "<0f>0<|box;x1=520;y1=380;x2=700;y2=440;w=800;h=500|><|mask;x=0;y=0;data=61,181,~!!!9I!0cZii%0$fi5DF.A#2{d^-_M1Y[oT*V_vIG8vban&'`=&xj{nwF|>";

    #[test]
    fn parses_real_line() {
        let b = parse_sam_text(REAL);
        assert_eq!(b, vec![SamBox { x1: 520, y1: 380, x2: 700, y2: 440, img_w: 800, img_h: 500 }]);
        assert_eq!(b[0].center(), (610, 410));
        assert_eq!(b[0].area(), 180 * 60);
    }

    #[test]
    fn several_hits_and_frames() {
        let t = "<0f>1<|box;x1=10;y1=20;x2=110;y2=60;w=1600;h=900|><|mask;x=0;y=0;data=ab|>\
                 2<|box;x1=300;y1=20;x2=400;y2=60;w=1600;h=900|><|mask;data=cd|>\n\
                 <1f>1<|box;x1=10;y1=20;x2=110;y2=60;w=1600;h=900|>";
        let b = parse_sam_text(t);
        assert_eq!(b.len(), 2);
        assert_eq!(b[1].x1, 300);
    }

    #[test]
    fn no_hit_and_garbage() {
        assert!(parse_sam_text("").is_empty());
        assert!(parse_sam_text("<0f>").is_empty());
        assert!(parse_sam_text("<|box;x1=1;y1=2|>").is_empty()); // missing keys
        assert!(parse_sam_text("<|box;x1=a;y1=2;x2=3;y2=4;w=10;h=10|>").is_empty());
        assert!(parse_sam_text("<|box;x1=5;y1=5;x2=5;y2=9;w=10;h=10|>").is_empty()); // zero width
        assert!(parse_sam_text("<|box;x1=1;y1=1;x2=5;y2=5;w=0;h=10|>").is_empty());
    }

    #[test]
    fn tolerant_order_floats_extra_keys_and_clamp() {
        let b = parse_sam_text("<|box;w=100;h=50;c=0.93;x2=120.4;y2=40.6;x1=10.2;y1=5|>");
        assert_eq!(b, vec![SamBox { x1: 10, y1: 5, x2: 100, y2: 41, img_w: 100, img_h: 50 }]);
        // swapped corners are normalised
        let b = parse_sam_text("<|box;x1=50;y1=40;x2=10;y2=5;w=100;h=50|>");
        assert_eq!((b[0].x1, b[0].y1, b[0].x2, b[0].y2), (10, 5, 50, 40));
    }

    #[test]
    fn phrases_add_fixed_synonyms() {
        assert_eq!(phrases("button"), ["button", "rounded button", "clickable button"]);
        assert_eq!(phrases(" Radio Button "), ["Radio Button", "small circle"]);
        assert_eq!(phrases("checkbox"), ["checkbox", "small square box"]);
        assert_eq!(phrases("text input field"), ["text input field", "input box"]);
        assert_eq!(phrases("link"), ["link", "underlined text"]);
        assert_eq!(phrases("icon"), ["icon", "small icon"]);
        assert_eq!(phrases("blue button"), ["blue button"], "only the whole query is matched");
        assert!(phrases("button").len() <= 3);
    }

    #[test]
    fn merge_drops_overlapping_boxes() {
        let b = |x1, y1, x2, y2| SamBox { x1, y1, x2, y2, img_w: 1000, img_h: 800 };
        let first = vec![b(100, 100, 300, 140), b(700, 600, 900, 640)];
        // The same button a little bigger (IoU ~0.8), a new one, and a box half over the first (IoU ~0.33).
        let second = vec![b(98, 98, 304, 142), b(400, 300, 500, 340), b(200, 100, 400, 140)];
        let m = merge_boxes(vec![first, second]);
        assert_eq!(m, vec![b(100, 100, 300, 140), b(700, 600, 900, 640), b(400, 300, 500, 340), b(200, 100, 400, 140)]);
        assert!((iou(&b(0, 0, 10, 10), &b(0, 0, 10, 10)) - 1.0).abs() < 1e-9);
        assert_eq!(iou(&b(0, 0, 10, 10), &b(20, 20, 30, 30)), 0.0);
        assert!(merge_boxes(vec![]).is_empty());
    }

    #[test]
    fn rescale_to_sent_image() {
        let b = SamBox { x1: 100, y1: 50, x2: 200, y2: 100, img_w: 1000, img_h: 500 };
        let r = rescale(b, 2000, 1000);
        assert_eq!((r.x1, r.y1, r.x2, r.y2, r.img_w, r.img_h), (200, 100, 400, 200, 2000, 1000));
        assert_eq!(rescale(b, 1000, 500), b);
    }
}
