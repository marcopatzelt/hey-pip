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
    fn garbage_is_error() {
        assert!(fit_image(b"not an image", 1600).is_err());
    }
}
