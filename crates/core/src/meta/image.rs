//! Prepare images for the API: decode PNG/JPEG, downscale so the long side is <= `max_side`,
//! encode PNG, and remember the factor back to the original pixels. OWNER: agent "meta".
//! (Screenshots use `platform::Frame::for_api`; this is for photos and files.)

use anyhow::{Context, Result};
use image::metadata::Orientation;
use image::{DynamicImage, GenericImageView, ImageDecoder, ImageFormat, ImageReader, imageops::FilterType};

use super::responses::Content;

/// Default long-side limit for images sent to Muse Spark / SAM.
pub const MAX_SIDE: u32 = 1600;

/// An image ready for the API.
#[derive(Debug, Clone)]
pub struct FittedImage {
    pub png: Vec<u8>,
    pub w: u32,
    pub h: u32,
    /// api pixel * factor = original pixel (1.0 if not scaled).
    pub factor: f32,
}

impl FittedImage {
    pub fn content(&self) -> Content {
        Content::png(&self.png)
    }
    /// API-image pixel -> original-image pixel.
    pub fn to_original(&self, x: u32, y: u32) -> (u32, u32) {
        ((x as f32 * self.factor).round() as u32, (y as f32 * self.factor).round() as u32)
    }
}

/// Decode `bytes` (PNG or JPEG), apply the EXIF orientation (phone photos are often stored
/// sideways), downscale if the long side exceeds `max_side`, encode PNG. `factor` maps to the
/// upright image. A PNG that already fits and needs no rotation is passed through unchanged.
pub fn fit_image(bytes: &[u8], max_side: u32) -> Result<FittedImage> {
    let format = image::guess_format(bytes).context("unknown image format")?;
    let mut decoder =
        ImageReader::with_format(std::io::Cursor::new(bytes), format).into_decoder().context("cannot read image")?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder).context("cannot read image")?;
    img.apply_orientation(orientation);
    let (w, h) = img.dimensions();
    let long = w.max(h);
    let max_side = max_side.max(1);
    if long <= max_side {
        let unchanged = format == ImageFormat::Png && orientation == Orientation::NoTransforms;
        let png = if unchanged { bytes.to_vec() } else { encode_png(&img)? };
        return Ok(FittedImage { png, w, h, factor: 1.0 });
    }
    let factor = long as f32 / max_side as f32;
    let nw = ((w as f32 / factor).round() as u32).max(1);
    let nh = ((h as f32 / factor).round() as u32).max(1);
    let small = img.resize_exact(nw, nh, FilterType::Triangle);
    Ok(FittedImage { png: encode_png(&small)?, w: nw, h: nh, factor })
}

/// A part of an image, enlarged: what the second grounding look sees. `x0, y0, w, h` = the region in source pixels;
/// the PNG is `w * scale` x `h * scale`.
#[derive(Debug, Clone)]
pub struct Zoom {
    pub png: Vec<u8>,
    pub x0: u32,
    pub y0: u32,
    pub w: u32,
    pub h: u32,
    pub scale: u32,
}

impl Zoom {
    /// Size of the enlarged PNG.
    pub fn size(&self) -> (u32, u32) {
        (self.w * self.scale, self.h * self.scale)
    }
    /// Pixel of the enlarged PNG -> pixel of the source image (clamped to the region).
    pub fn to_source(&self, x: u32, y: u32) -> (u32, u32) {
        (self.x0 + (x / self.scale).min(self.w - 1), self.y0 + (y / self.scale).min(self.h - 1))
    }
    /// Pixel of the source image -> pixel of the enlarged PNG (clamped to it).
    pub fn from_source(&self, x: u32, y: u32) -> (u32, u32) {
        let (zw, zh) = self.size();
        ((x.saturating_sub(self.x0) * self.scale).min(zw), (y.saturating_sub(self.y0) * self.scale).min(zh))
    }
}

/// The `rw` x `rh` region centred on (cx, cy), moved inside an `img_w` x `img_h` image at the edges (and cut to the
/// image when it is smaller): (x0, y0, w, h).
pub fn region_around(img_w: u32, img_h: u32, cx: u32, cy: u32, rw: u32, rh: u32) -> (u32, u32, u32, u32) {
    let (w, h) = (rw.clamp(1, img_w.max(1)), rh.clamp(1, img_h.max(1)));
    let x0 = cx.saturating_sub(w / 2).min(img_w.saturating_sub(w));
    let y0 = cy.saturating_sub(h / 2).min(img_h.saturating_sub(h));
    (x0, y0, w, h)
}

/// Cut the `rw` x `rh` region around (cx, cy) out of a PNG and enlarge it `scale` times. Every pixel becomes a
/// `scale` x `scale` block: edges and small letters stay sharp, and it is fast.
pub fn zoom_png(png: &[u8], cx: u32, cy: u32, rw: u32, rh: u32, scale: u32) -> Result<Zoom> {
    use image::ImageEncoder;
    use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
    // Screens are RGB, test pictures often RGBA: both are used as they are (a conversion costs more than the rest).
    let (iw, ih, raw, color) = match image::load_from_memory(png).context("cannot read image")? {
        DynamicImage::ImageRgb8(i) => (i.width(), i.height(), i.into_raw(), image::ExtendedColorType::Rgb8),
        DynamicImage::ImageRgba8(i) => (i.width(), i.height(), i.into_raw(), image::ExtendedColorType::Rgba8),
        other => {
            let i = other.into_rgb8();
            (i.width(), i.height(), i.into_raw(), image::ExtendedColorType::Rgb8)
        }
    };
    anyhow::ensure!(iw > 0 && ih > 0, "empty image");
    let bpp = if color == image::ExtendedColorType::Rgba8 { 4 } else { 3 };
    let scale = scale.max(1);
    let (x0, y0, w, h) = region_around(iw, ih, cx.min(iw - 1), cy.min(ih - 1), rw, rh);
    // Each source row is written `scale` times, each pixel `scale` times (plain loops: quick in debug builds too).
    let mut big = Vec::with_capacity((w * scale * h * scale) as usize * bpp);
    let mut row = Vec::with_capacity((w * scale) as usize * bpp);
    for y in y0..y0 + h {
        let start = (y * iw + x0) as usize * bpp;
        row.clear();
        for px in raw[start..start + w as usize * bpp].chunks_exact(bpp) {
            for _ in 0..scale {
                row.extend_from_slice(px);
            }
        }
        for _ in 0..scale {
            big.extend_from_slice(&row);
        }
    }
    let mut out = Vec::with_capacity(big.len() / 8);
    PngEncoder::new_with_quality(&mut out, CompressionType::Fast, PngFilter::Adaptive).write_image(
        &big,
        w * scale,
        h * scale,
        color,
    )?;
    Ok(Zoom { png: out, x0, y0, w, h, scale })
}

/// Mean colour change (0..255) between two PNGs of the same size over the WHOLE picture (every second pixel), or
/// None when they differ in size or cannot be read. A scroll that moves content only at an edge (a form at the left
/// of a wide screen) counts as a change too; a blinking caret or a clock gives ~0.03.
pub fn mean_diff(a: &[u8], b: &[u8]) -> Option<f32> {
    let (a, b) = (image::load_from_memory(a).ok()?.into_rgb8(), image::load_from_memory(b).ok()?.into_rgb8());
    if a.dimensions() != b.dimensions() || a.width() == 0 || a.height() == 0 {
        return None;
    }
    let (mut sum, mut n) = (0u64, 0u64);
    for y in (0..a.height()).step_by(2) {
        for x in (0..a.width()).step_by(2) {
            let (p, q) = (a.get_pixel(x, y), b.get_pixel(x, y));
            sum += (0..3).map(|i| u64::from(p[i].abs_diff(q[i]))).sum::<u64>();
            n += 3;
        }
    }
    Some(sum as f32 / n as f32)
}

fn encode_png(img: &DynamicImage) -> Result<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    // Drop alpha only if the image has none to keep files small.
    let img = if img.color().has_alpha() { img.clone() } else { DynamicImage::ImageRgb8(img.to_rgb8()) };
    img.write_to(&mut out, ImageFormat::Png)?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        encode_png(&DynamicImage::ImageRgb8(image::RgbImage::from_pixel(w, h, image::Rgb([200, 10, 10])))).unwrap()
    }

    #[test]
    fn small_png_passes_through() {
        let p = png(300, 200);
        let f = fit_image(&p, MAX_SIDE).unwrap();
        assert_eq!((f.w, f.h, f.factor), (300, 200, 1.0));
        assert_eq!(f.png, p);
    }

    #[test]
    fn large_is_downscaled_with_factor() {
        let f = fit_image(&png(3200, 1800), 1600).unwrap();
        assert_eq!((f.w, f.h), (1600, 900));
        assert!((f.factor - 2.0).abs() < 1e-6);
        assert_eq!(f.to_original(800, 450), (1600, 900));
        let back = image::load_from_memory(&f.png).unwrap();
        assert_eq!(back.dimensions(), (1600, 900));
        assert!(matches!(f.content(), Content::Image { data_url } if data_url.starts_with("data:image/png;base64,")));
    }

    #[test]
    fn portrait_jpeg() {
        let mut jpg = std::io::Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(image::RgbImage::new(1000, 4000)).write_to(&mut jpg, ImageFormat::Jpeg).unwrap();
        let f = fit_image(jpg.get_ref(), 1600).unwrap();
        assert_eq!((f.w, f.h), (400, 1600));
        assert!((f.factor - 2.5).abs() < 1e-6);
        assert_eq!(image::guess_format(&f.png).unwrap(), ImageFormat::Png);
    }

    /// JPEG with an EXIF APP1 segment saying "rotate 90° clockwise" (orientation 6).
    fn jpeg_rotated_90(w: u32, h: u32) -> Vec<u8> {
        let mut jpg = std::io::Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(image::RgbImage::new(w, h)).write_to(&mut jpg, ImageFormat::Jpeg).unwrap();
        let jpg = jpg.into_inner();
        let mut tiff = b"II*\0\x08\0\0\0\x01\0".to_vec();
        tiff.extend_from_slice(&[0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0]);
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(&tiff);
        let len = (app1.len() + 2) as u16;
        let mut out = jpg[..2].to_vec();
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&app1);
        out.extend_from_slice(&jpg[2..]);
        out
    }

    #[test]
    fn exif_orientation_is_applied() {
        let f = fit_image(&jpeg_rotated_90(300, 100), 1600).unwrap();
        assert_eq!((f.w, f.h), (100, 300));
        let f = fit_image(&jpeg_rotated_90(3200, 800), 1600).unwrap();
        assert_eq!((f.w, f.h), (400, 1600));
        assert!((f.factor - 2.0).abs() < 1e-6);
    }

    #[test]
    fn zoom_region_stays_inside_and_maps_back() {
        // Middle: centred.
        assert_eq!(region_around(1600, 1000, 800, 500, 480, 300), (560, 350, 480, 300));
        // Edges: moved inside, never cut.
        assert_eq!(region_around(1600, 1000, 5, 3, 480, 300), (0, 0, 480, 300));
        assert_eq!(region_around(1600, 1000, 1599, 999, 480, 300), (1120, 700, 480, 300));
        // An image smaller than the region: the whole image.
        assert_eq!(region_around(300, 200, 150, 100, 480, 300), (0, 0, 300, 200));

        let z = zoom_png(&png(1600, 1000), 1599, 999, 480, 300, 2).unwrap();
        assert_eq!((z.x0, z.y0, z.w, z.h), (1120, 700, 480, 300));
        assert_eq!(z.size(), (960, 600));
        assert_eq!(image::load_from_memory(&z.png).unwrap().dimensions(), (960, 600));
        // Corners of the enlarged picture map to the region's corners in the source.
        assert_eq!(z.to_source(0, 0), (1120, 700));
        assert_eq!(z.to_source(959, 599), (1599, 999));
        assert_eq!(z.to_source(5000, 5000), (1599, 999), "clamped");
        assert_eq!(z.to_source(441, 187), (1120 + 220, 700 + 93));
        assert_eq!(z.from_source(1120 + 220, 700 + 93), (440, 186));
        assert_eq!(z.from_source(0, 0), (0, 0));

        let z = zoom_png(&png(1600, 1000), 0, 0, 480, 300, 2).unwrap();
        assert_eq!((z.x0, z.y0), (0, 0));
        assert_eq!(z.to_source(959, 599), (479, 299));
    }

    #[test]
    fn mean_diff_sees_a_change_at_the_edge() {
        let plain = png(1600, 1000);
        assert_eq!(mean_diff(&plain, &plain), Some(0.0));
        // A form at the left edge moved up: the middle of the screen stays blank, the change is still seen.
        let mut moved = image::RgbImage::from_pixel(1600, 1000, image::Rgb([200, 10, 10]));
        for y in 0..200 {
            for x in 0..300 {
                moved.put_pixel(x, y, image::Rgb([20, 20, 20]));
            }
        }
        let moved = encode_png(&DynamicImage::ImageRgb8(moved)).unwrap();
        assert!(mean_diff(&plain, &moved).unwrap() > 2.0);
        assert_eq!(mean_diff(&plain, &png(300, 200)), None, "other size");
        assert_eq!(mean_diff(&plain, b"junk"), None);
    }

    #[test]
    fn garbage_is_error() {
        assert!(fit_image(b"not an image", 1600).is_err());
    }
}
