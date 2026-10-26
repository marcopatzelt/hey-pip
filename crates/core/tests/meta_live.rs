//! Live tests against the real Meta Model API. Cost: a few cents per full run.
//! Run: `cargo test -p pip-core --test meta_live -- --ignored --nocapture`
//! Needs MODEL_API_KEY in the environment or in the repo's .env. OWNER: agent "meta".
//! Optional: ASR_TEST_WAV=<path to a mono 16 kHz WAV with speech>.

use futures_util::StreamExt;
use image::{Rgb, RgbImage};
use pip_core::i18n::Lang;
use pip_core::meta::{
    Content, Effort, InputItem, MODEL_SPARK, MetaClient, ResponseEvent, ResponseRequest, ToolDef, fit_image,
};
use serde_json::json;

fn client() -> MetaClient {
    MetaClient::from_env().expect("MODEL_API_KEY missing (.env)")
}

fn png(img: &RgbImage) -> Vec<u8> {
    let mut c = std::io::Cursor::new(Vec::new());
    img.write_to(&mut c, image::ImageFormat::Png).unwrap();
    c.into_inner()
}

fn fill(img: &mut RgbImage, x: u32, y: u32, w: u32, h: u32, c: Rgb<u8>) {
    for yy in y..y + h {
        for xx in x..x + w {
            img.put_pixel(xx, yy, c);
        }
    }
}

/// 800x500 invented form (ImageMagick): two input fields, a blue rounded "Absenden" button
/// at (520,380)-(700,440). A flat rectangle without text is NOT found as "button" by SAM.
fn form_png() -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/meta_form.png")).unwrap()
}

#[tokio::test]
#[ignore = "real API call"]
async fn live_respond_text() {
    let c = client();
    let req = ResponseRequest::new(
        MODEL_SPARK,
        "Antworte mit genau einem Wort.",
        vec![InputItem::user(vec![Content::text("Welche Farbe hat der Himmel an einem klaren Tag?")])],
    );
    let out = c.respond(&req).await.unwrap();
    println!("text={:?} usage={:?}", out.text, out.usage);
    assert!(out.text.to_lowercase().contains("blau"));
    assert!(out.usage.input_tokens > 0);
}

#[tokio::test]
#[ignore = "real API call"]
async fn live_function_roundtrip() {
    let c = client();
    let tools = vec![ToolDef {
        name: "lookup_deadline".into(),
        description: "Look up the due date of a stored deadline by its title.".into(),
        parameters: json!({"type":"object","properties":{"title":{"type":"string"}},"required":["title"],"additionalProperties":false}),
    }];
    let instructions = "You help with paperwork. Always use lookup_deadline to answer questions about deadlines. Answer in one short German sentence.";
    let mut req = ResponseRequest::new(
        MODEL_SPARK,
        instructions,
        vec![InputItem::user(vec![Content::text("Bis wann muss die Folge-Krankschreibung da sein?")])],
    );
    req.tools = tools;
    let first = c.respond(&req).await.unwrap();
    println!("first: calls={:?}", first.calls);
    assert!(!first.calls.is_empty(), "model did not call the tool");
    assert_eq!(first.calls[0].name, "lookup_deadline");
    req.input = first.calls.iter().map(|call| call.output("2026-11-02 (Montag)")).collect();
    req.previous_response_id = Some(first.id.clone());
    let second = c.respond(&req).await.unwrap();
    println!("second: text={:?}", second.text);
    assert!(second.calls.is_empty());
    assert!(second.text.contains('2') && (second.text.contains("Nov") || second.text.contains("11")));
    println!("usage total {:?}", c.usage_total());
}

#[tokio::test]
#[ignore = "real API call"]
async fn live_stream() {
    let c = client();
    let mut req = ResponseRequest::new(
        MODEL_SPARK,
        "Antworte kurz.",
        vec![InputItem::user(vec![Content::text("Zähle von 1 bis 5, getrennt durch Kommas.")])],
    );
    req.effort = Effort::Low;
    let mut deltas = String::new();
    let mut completed = None;
    let mut s = std::pin::pin!(c.respond_stream(&req));
    while let Some(ev) = s.next().await {
        match ev.unwrap() {
            ResponseEvent::TextDelta(d) => deltas.push_str(&d),
            ResponseEvent::Completed(o) => completed = Some(o),
            ResponseEvent::Call(_) => {}
        }
    }
    let completed = completed.expect("no response.completed");
    println!("deltas={deltas:?} id={}", completed.id);
    assert!(deltas.contains('5'));
    assert!(c.usage_total().output_tokens > 0);
}

#[tokio::test]
#[ignore = "real API call"]
async fn live_image_input_downscaled() {
    let c = client();
    // 3200x2000 red image with a blue square in the top left quarter.
    let mut img = RgbImage::from_pixel(3200, 2000, Rgb([220, 30, 30]));
    fill(&mut img, 200, 200, 800, 600, Rgb([20, 60, 230]));
    let fitted = fit_image(&png(&img), 1600).unwrap();
    assert_eq!((fitted.w, fitted.h), (1600, 1000));
    assert!((fitted.factor - 2.0).abs() < 1e-6);
    let req = ResponseRequest::new(
        MODEL_SPARK,
        "Answer with one lowercase English word.",
        vec![InputItem::user(vec![
            Content::text("What is the color of the small square in the top left?"),
            fitted.content(),
        ])],
    );
    let out = c.respond(&req).await.unwrap();
    println!("answer={:?} usage={:?}", out.text, out.usage);
    assert!(out.text.to_lowercase().contains("blue"));
}

#[tokio::test]
#[ignore = "real API call"]
async fn live_sam_finds_button() {
    let c = client();
    let t = std::time::Instant::now();
    let boxes = c.sam_segment(&form_png(), "button").await.unwrap();
    println!("sam boxes={boxes:?} in {:?}", t.elapsed());
    assert!(!boxes.is_empty());
    let hit = boxes.iter().any(|b| {
        let (x, y) = b.center();
        (520..700).contains(&x) && (380..440).contains(&y)
    });
    assert!(hit, "no box on the blue button");
    let none = c.sam_segment(&form_png(), "elephant").await.unwrap();
    println!("sam 'elephant' boxes={none:?}");
}

#[tokio::test]
#[ignore = "real API call"]
async fn live_transcribe() {
    let c = client();
    let (wav, has_speech) = match std::env::var("ASR_TEST_WAV") {
        Ok(p) => (std::fs::read(p).unwrap(), true),
        Err(_) => (pip_core::meta::wav_from_pcm16(&vec![0i16; 16_000], 16_000).unwrap(), false),
    };
    let t = c.transcribe(wav, Lang::De).await.unwrap();
    println!("transcript={t:?}");
    if has_speech {
        assert!(t.to_lowercase().contains("krankschreibung"));
    }
}

#[tokio::test]
#[ignore = "real API call ($0.02)"]
async fn live_generate_and_edit_image() {
    let c = client();
    let img =
        c.generate_image("a tiny flat blue water drop icon on plain white background", "1024x1024").await.unwrap();
    assert_eq!(&img[..8], b"\x89PNG\r\n\x1a\n");
    let dims = image::load_from_memory(&img).unwrap();
    println!("generated {}x{} ({} bytes)", dims.width(), dims.height(), img.len());
    let small = fit_image(&img, 512).unwrap();
    let edited = c.edit_image("make the drop purple", &small.png).await.unwrap();
    assert_eq!(&edited[..8], b"\x89PNG\r\n\x1a\n");
    println!("edited {} bytes", edited.len());
}
