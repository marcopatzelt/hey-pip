//! `sam` (SAM 3.1 boxes on an image) and `ground` (the agent's full target search).

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use pip_core::agent::{API_MAX_SIDE, Agent, Gate};
use pip_core::events::Bus;
use pip_core::meta::SamBox;
use pip_core::platform::{ApiImage, Frame, PhysPoint};
use pip_platform::fake::FakePlatform;

use crate::Ctx;
use crate::letters::write_file;

/// The image as the agent would send it (long side max 1600 px).
pub fn api_image(path: &Path) -> Result<ApiImage> {
    let img = image::open(path).with_context(|| format!("cannot open image {}", path.display()))?.into_rgba8();
    Frame { image: img, origin: PhysPoint { x: 0, y: 0 }, scale: 1.0 }.for_api(API_MAX_SIDE)
}

pub async fn sam(ctx: &Ctx, image: &Path, query: &str, out: &Path) -> Result<()> {
    let api = api_image(image)?;
    let meta = ctx.meta()?;
    let started = Instant::now();
    let boxes = meta.sam_segment(&api.png, query).await?;
    let secs = started.elapsed().as_secs_f32();
    println!(
        "SAM 3.1 \"{query}\" on {} ({}x{} sent): {} box(es) in {secs:.1} s",
        image.display(),
        api.w,
        api.h,
        boxes.len()
    );
    for (i, b) in boxes.iter().enumerate() {
        let r = api.box_to_phys(b);
        let (cx, cy) = b.center();
        let c = api.to_phys(cx, cy);
        println!("  #{} x={} y={} w={} h={} (image pixels)  center ({}, {})", i + 1, r.x, r.y, r.w, r.h, c.x, c.y);
    }
    write_file(out, &api.with_numbered_boxes(&boxes)?)?;
    println!("Annotated PNG: {}", out.display());
    Ok(())
}

pub async fn ground(ctx: &Ctx, image: &Path, description: &str, query: &str, out: &Path) -> Result<()> {
    let api = api_image(image)?;
    let started = Instant::now();
    let point = ground_point(ctx, image, description, query).await?;
    let secs = started.elapsed().as_secs_f32();
    match point {
        Some(p) => {
            println!("\"{description}\" (SAM query \"{query}\"): click at ({}, {}) in {secs:.1} s", p.x, p.y);
            let r = 12;
            let x = (p.x.max(0) as f32 / api.factor) as u32;
            let y = (p.y.max(0) as f32 / api.factor) as u32;
            let mark = SamBox {
                x1: x.saturating_sub(r),
                y1: y.saturating_sub(r),
                x2: (x + r).min(api.w),
                y2: (y + r).min(api.h),
                img_w: api.w,
                img_h: api.h,
            };
            write_file(out, &api.with_numbered_boxes(&[mark])?)?;
            println!("Marked PNG: {}", out.display());
        }
        None => println!("\"{description}\": not found ({secs:.1} s)"),
    }
    Ok(())
}

/// Shared by eval: the grounding call alone, on a prepared image.
pub async fn ground_point(ctx: &Ctx, image: &Path, description: &str, query: &str) -> Result<Option<PhysPoint>> {
    let api = api_image(image)?;
    let (_fake, platform) = FakePlatform::from_png(image)?;
    let (bus, _rx) = Bus::new();
    let agent = Agent::new(ctx.meta()?, platform, ctx.memory.clone(), Gate::new(true), bus, ctx.clock.clone())
        .with_pace(Duration::ZERO);
    agent.ground(&api, query, description).await
}
