//! Platform traits and screen-image helpers. Implementations live in crates/platform.
//! All coordinates are PHYSICAL pixels of the primary monitor (one monitor only).
//!
//! OWNER: agent "platform" (crates/platform/**, crates/core/src/platform.rs).
//! Types/traits are frozen; fill in the `impl` bodies.

use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::meta::SamBox;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysPoint {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl PhysRect {
    pub fn contains(&self, p: PhysPoint) -> bool {
        p.x >= self.x && p.y >= self.y && p.x < self.x + self.w && p.y < self.y + self.h
    }
}

/// A captured screen. `origin` = monitor offset in the virtual desktop, `scale` = DPI scale (1.0 = 96 dpi).
#[derive(Debug, Clone)]
pub struct Frame {
    pub image: image::RgbaImage,
    pub origin: PhysPoint,
    pub scale: f32,
}

/// The screenshot as sent to the API: downscaled PNG plus the way back to physical pixels.
#[derive(Debug, Clone)]
pub struct ApiImage {
    pub png: Vec<u8>,
    pub w: u32,
    pub h: u32,
    /// api pixel * factor = frame pixel
    pub factor: f32,
    pub origin: PhysPoint,
}

impl Frame {
    /// Grey out these rects (Pip, bubble, card) so the model does not act on our own UI.
    /// Rects are physical (virtual-desktop) coordinates; subtract `origin`.
    pub fn mask(&mut self, rects: &[PhysRect]) {
        let (iw, ih) = (self.image.width() as i32, self.image.height() as i32);
        for r in rects {
            let x0 = (r.x - self.origin.x).clamp(0, iw);
            let y0 = (r.y - self.origin.y).clamp(0, ih);
            let x1 = (r.x - self.origin.x).saturating_add(r.w.max(0)).clamp(0, iw);
            let y1 = (r.y - self.origin.y).saturating_add(r.h.max(0)).clamp(0, ih);
            for y in y0..y1 {
                for x in x0..x1 {
                    self.image.put_pixel(x as u32, y as u32, MASK_GREY);
                }
            }
        }
    }

    /// Downscale so the long side is <= max_side (use 1600), encode PNG.
    pub fn for_api(&self, max_side: u32) -> Result<ApiImage> {
        let (w, h) = self.image.dimensions();
        anyhow::ensure!(w > 0 && h > 0, "empty screenshot");
        let long = w.max(h);
        let max_side = max_side.max(1);
        let (png, aw, ah, factor) = if long <= max_side {
            (encode_png(&self.image)?, w, h, 1.0)
        } else {
            let factor = long as f32 / max_side as f32;
            let aw = ((w as f32 / factor).round() as u32).clamp(1, max_side);
            let ah = ((h as f32 / factor).round() as u32).clamp(1, max_side);
            let small = image::imageops::resize(&self.image, aw, ah, image::imageops::FilterType::Triangle);
            (encode_png(&small)?, aw, ah, factor)
        };
        Ok(ApiImage { png, w: aw, h: ah, factor, origin: self.origin })
    }
}

/// Colour used to hide our own UI in screenshots.
const MASK_GREY: image::Rgba<u8> = image::Rgba([128, 128, 128, 255]);

/// Encode as RGB PNG (screens are opaque; RGB is smaller), fast compression.
fn encode_png(img: &image::RgbaImage) -> Result<Vec<u8>> {
    use image::ImageEncoder;
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    let rgb = image::DynamicImage::ImageRgba8(img.clone()).into_rgb8();
    let mut out = Vec::with_capacity((rgb.width() * rgb.height()) as usize / 2);
    PngEncoder::new_with_quality(&mut out, CompressionType::Fast, FilterType::Adaptive).write_image(
        rgb.as_raw(),
        rgb.width(),
        rgb.height(),
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(out)
}

fn decode_png(png: &[u8]) -> Result<image::RgbaImage> {
    Ok(image::load_from_memory_with_format(png, image::ImageFormat::Png)?.into_rgba8())
}

impl ApiImage {
    /// API-image pixel -> physical point (x * factor + origin).
    pub fn to_phys(&self, x: u32, y: u32) -> PhysPoint {
        PhysPoint {
            x: self.origin.x + (x as f32 * self.factor).round() as i32,
            y: self.origin.y + (y as f32 * self.factor).round() as i32,
        }
    }

    /// Box corners in this image's pixels (SAM reports its own image size in w/h; rescale if it differs).
    /// Returns (x1, y1, x2, y2) clamped to the image, x2 > x1 and y2 > y1 unless the box is empty.
    fn box_px(&self, b: &SamBox) -> (u32, u32, u32, u32) {
        let sx = if b.img_w > 0 { self.w as f32 / b.img_w as f32 } else { 1.0 };
        let sy = if b.img_h > 0 { self.h as f32 / b.img_h as f32 } else { 1.0 };
        let (ax, bx) = (b.x1.min(b.x2), b.x1.max(b.x2));
        let (ay, by) = (b.y1.min(b.y2), b.y1.max(b.y2));
        let f = |v: u32, s: f32, max: u32| ((v as f32 * s).round() as u32).min(max);
        (f(ax, sx, self.w), f(ay, sy, self.h), f(bx, sx, self.w), f(by, sy, self.h))
    }

    /// Crop a box (+pad px) out of the API image as PNG (approval-card thumbnail).
    pub fn crop_png(&self, b: &SamBox, pad: u32) -> Result<Vec<u8>> {
        let img = decode_png(&self.png)?;
        let (x1, y1, x2, y2) = self.box_px(b);
        let x0 = x1.saturating_sub(pad);
        let y0 = y1.saturating_sub(pad);
        let xe = (x2 + pad).min(img.width());
        let ye = (y2 + pad).min(img.height());
        anyhow::ensure!(xe > x0 && ye > y0, "box is outside the screenshot");
        let crop = image::imageops::crop_imm(&img, x0, y0, xe - x0, ye - y0).to_image();
        encode_png(&crop)
    }

    /// Draw each box with a big readable number 1..n (bitmap digits, high contrast) -> PNG.
    pub fn with_numbered_boxes(&self, boxes: &[SamBox]) -> Result<Vec<u8>> {
        let mut img = decode_png(&self.png)?;
        // Scale lines and digits with the image so numbers stay readable after the model downsizes.
        let unit = (self.w.max(self.h) / 400).clamp(2, 6);
        for (i, b) in boxes.iter().enumerate() {
            let colour = BOX_COLOURS[i % BOX_COLOURS.len()];
            let (x1, y1, x2, y2) = self.box_px(b);
            draw::rect_outline(&mut img, x1 as i32, y1 as i32, x2 as i32, y2 as i32, unit as i32, colour);
            draw::label(&mut img, x1 as i32, y1 as i32, &(i + 1).to_string(), unit, colour);
        }
        encode_png(&img)
    }

    /// Mean absolute colour difference (0..255) inside the middle half of a box, between this image and
    /// a later one of the same size. The rim is left out: Pip's highlight ring sits just outside the box.
    /// None if the images differ in size or the box is empty.
    pub fn region_diff(&self, later: &ApiImage, b: &SamBox) -> Option<f32> {
        if (self.w, self.h) != (later.w, later.h) {
            return None;
        }
        let (a, c) = (decode_png(&self.png).ok()?, decode_png(&later.png).ok()?);
        let (x1, y1, x2, y2) = self.box_px(b);
        let (iw, ih) = ((x2 - x1) / 4, (y2 - y1) / 4);
        let (x1, y1, x2, y2) =
            (x1 + iw, y1 + ih, x2.saturating_sub(iw).min(a.width()), y2.saturating_sub(ih).min(a.height()));
        if x2 <= x1 || y2 <= y1 {
            return None;
        }
        let mut sum = 0u64;
        for y in y1..y2 {
            for x in x1..x2 {
                let (p, q) = (a.get_pixel(x, y), c.get_pixel(x, y));
                sum += (0..3).map(|i| u64::from(p[i].abs_diff(q[i]))).sum::<u64>();
            }
        }
        Some(sum as f32 / (3 * (x2 - x1) * (y2 - y1)) as f32)
    }

    /// Physical rect -> box in this image's pixels (UI-tree rects to card thumbnails).
    pub fn phys_to_box(&self, r: &PhysRect) -> SamBox {
        let f = |v: i32, o: i32, max: u32| (((v - o) as f32 / self.factor).round().max(0.0) as u32).min(max);
        SamBox {
            x1: f(r.x, self.origin.x, self.w),
            y1: f(r.y, self.origin.y, self.h),
            x2: f(r.x + r.w, self.origin.x, self.w),
            y2: f(r.y + r.h, self.origin.y, self.h),
            img_w: self.w,
            img_h: self.h,
        }
    }

    /// Map a box from API-image pixels to a physical rect (for PointAt highlights).
    pub fn box_to_phys(&self, b: &SamBox) -> PhysRect {
        let (x1, y1, x2, y2) = self.box_px(b);
        let a = self.to_phys(x1, y1);
        let c = self.to_phys(x2, y2);
        PhysRect { x: a.x, y: a.y, w: (c.x - a.x).max(1), h: (c.y - a.y).max(1) }
    }
}

/// Saturated colours that stand out on typical light and dark UIs.
const BOX_COLOURS: [image::Rgba<u8>; 4] = [
    image::Rgba([255, 0, 200, 255]),
    image::Rgba([0, 170, 255, 255]),
    image::Rgba([255, 120, 0, 255]),
    image::Rgba([0, 200, 80, 255]),
];

/// Tiny raster helpers (no imageproc): box outlines and number labels with a 5x7 bitmap font.
mod draw {
    use image::{Rgba, RgbaImage};

    const BLACK: Rgba<u8> = Rgba([0, 0, 0, 255]);
    const WHITE: Rgba<u8> = Rgba([255, 255, 255, 255]);

    /// 5x7 digits, one row per byte, bit 4 = leftmost column.
    const DIGITS: [[u8; 7]; 10] = [
        [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E], // 0
        [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E], // 1
        [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F], // 2
        [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E], // 3
        [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02], // 4
        [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E], // 5
        [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E], // 6
        [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08], // 7
        [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E], // 8
        [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C], // 9
    ];

    pub fn fill(img: &mut RgbaImage, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgba<u8>) {
        let (w, h) = (img.width() as i32, img.height() as i32);
        for y in y0.clamp(0, h)..y1.clamp(0, h) {
            for x in x0.clamp(0, w)..x1.clamp(0, w) {
                img.put_pixel(x as u32, y as u32, c);
            }
        }
    }

    /// Coloured frame of thickness `t` with a thin black edge on both sides for contrast.
    pub fn rect_outline(img: &mut RgbaImage, x1: i32, y1: i32, x2: i32, y2: i32, t: i32, c: Rgba<u8>) {
        for (off, col) in [(t + 1, BLACK), (t, c)] {
            let (ax, ay, bx, by) = (x1 - off, y1 - off, x2 + off, y2 + off);
            let k = if col == BLACK { t + 2 } else { t };
            fill(img, ax, ay, bx, ay + k, col);
            fill(img, ax, by - k, bx, by, col);
            fill(img, ax, ay, ax + k, by, col);
            fill(img, bx - k, ay, bx, by, col);
        }
    }

    /// Number label: black digits on a coloured plate with a black border, at the box's top-left,
    /// moved inside the image when the box touches an edge.
    pub fn label(img: &mut RgbaImage, x: i32, y: i32, text: &str, unit: u32, c: Rgba<u8>) {
        let s = unit as i32 * 2; // pixel size of one font dot
        let pad = s;
        let tw = text.len() as i32 * 6 * s - s;
        let th = 7 * s;
        let (pw, ph) = (tw + 2 * pad, th + 2 * pad);
        let (w, h) = (img.width() as i32, img.height() as i32);
        // Prefer above the box; else inside its top-left corner.
        let mut px = x;
        let mut py = if y - ph - 2 >= 0 { y - ph - 2 } else { y + 2 };
        px = px.clamp(0, (w - pw).max(0));
        py = py.clamp(0, (h - ph).max(0));
        fill(img, px - 2, py - 2, px + pw + 2, py + ph + 2, BLACK);
        fill(img, px, py, px + pw, py + ph, c);
        let ink = if luminance(c) > 140 { BLACK } else { WHITE };
        for (i, ch) in text.chars().enumerate() {
            let Some(d) = ch.to_digit(10) else { continue };
            let ox = px + pad + i as i32 * 6 * s;
            for (row, bits) in DIGITS[d as usize].iter().enumerate() {
                for col in 0..5 {
                    if bits & (0x10 >> col) != 0 {
                        let dx = ox + col * s;
                        let dy = py + pad + row as i32 * s;
                        fill(img, dx, dy, dx + s, dy + s, ink);
                    }
                }
            }
        }
    }

    fn luminance(c: Rgba<u8>) -> u32 {
        (299 * c[0] as u32 + 587 * c[1] as u32 + 114 * c[2] as u32) / 1000
    }
}

pub trait Screen: Send + Sync {
    fn capture_primary(&self) -> Result<Frame>;
    /// True when screenshots never show Pip's overlay (Windows: excluded from capture), so no masks are needed.
    fn hides_overlay(&self) -> bool {
        false
    }
}

pub trait Input: Send + Sync {
    fn move_to(&self, p: PhysPoint) -> Result<()>;
    fn click(&self, p: PhysPoint) -> Result<()>;
    fn type_text(&self, s: &str) -> Result<()>;
    /// "enter", "tab", "esc", "ctrl+a", "alt+f4" ...
    fn key(&self, combo: &str) -> Result<()>;
    fn scroll(&self, p: PhysPoint, dy: i32) -> Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiElement {
    pub name: String,
    pub control: String,
    pub is_password: bool,
}

impl UiElement {
    /// Buttons and links: Space or Enter on them activates them.
    pub fn is_pressable(&self) -> bool {
        matches!(self.control.as_str(), "Button" | "Hyperlink" | "SplitButton" | "MenuItem")
    }
    /// Places where typed text goes.
    pub fn is_text_input(&self) -> bool {
        matches!(self.control.as_str(), "Edit" | "Document" | "ComboBox")
    }
}

/// One named element of the page in front, as the OS reports it (Windows UI Automation). The agent lists
/// these to the model ("#3 edit \"Full name\" = \"\"") and acts by number, without finding things in the picture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutlineItem {
    /// 1.. in document order, unique within one outline.
    pub id: u32,
    /// edit, button, checkbox, radio, combo, link, item, menuitem, tab, text
    pub role: String,
    pub name: String,
    /// What a field shows (max 60 chars); "checked"/"unchecked" for boxes. Never set for password fields.
    #[serde(default)]
    pub value: Option<String>,
    pub rect: PhysRect,
    /// Scrolled out of view (or outside the window).
    #[serde(default)]
    pub offscreen: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub is_password: bool,
    /// The OS's own words for the control ("Datumsauswahl", "date picker", "Eingabefeld"); None = unknown.
    #[serde(default)]
    pub localized_role: Option<String>,
    /// A date input (Edge/Chrome `<input type=date>`): its day, month and year are separate parts
    /// (`UiTree::date_parts`), and it ignores text set through the UI tree.
    #[serde(default)]
    pub is_date: bool,
}

fn yes() -> bool {
    true
}

impl OutlineItem {
    /// The UI Automation control name for this role (what `UiElement::control` holds).
    pub fn control(&self) -> &'static str {
        match self.role.as_str() {
            "edit" => "Edit",
            "button" => "Button",
            "checkbox" => "CheckBox",
            "radio" => "RadioButton",
            "combo" => "ComboBox",
            "link" => "Hyperlink",
            "item" => "ListItem",
            "menuitem" => "MenuItem",
            "tab" => "TabItem",
            "spinner" => "Spinner",
            _ => "Text",
        }
    }

    pub fn as_element(&self) -> UiElement {
        UiElement { name: self.name.clone(), control: self.control().to_string(), is_password: self.is_password }
    }

    pub fn center(&self) -> PhysPoint {
        PhysPoint { x: self.rect.x + self.rect.w / 2, y: self.rect.y + self.rect.h / 2 }
    }

    /// Text goes in here (edit fields and editable combo boxes).
    pub fn takes_text(&self) -> bool {
        matches!(self.role.as_str(), "edit" | "combo")
    }

    /// A date input: flagged by the platform, or its localized role names a date.
    pub fn looks_like_date(&self) -> bool {
        self.is_date || self.localized_role.as_deref().is_some_and(is_date_role)
    }
}

/// True if a localized control type names a date control: "Datumsauswahl", "date picker", "Datums- und
/// Uhrzeitauswahl", "sélecteur de date". Never "update" or "Eingabefeld".
pub fn is_date_role(localized: &str) -> bool {
    let l = localized.to_lowercase();
    l.contains("datum") || l.split(|c: char| !c.is_alphanumeric()).any(|w| w == "date")
}

/// Role name of a UI Automation control type name ("Edit" -> "edit"); None = not listed in an outline.
pub fn outline_role(control: &str) -> Option<&'static str> {
    Some(match control {
        "Edit" => "edit",
        "Button" | "SplitButton" => "button",
        "CheckBox" => "checkbox",
        "RadioButton" => "radio",
        "ComboBox" => "combo",
        "Hyperlink" => "link",
        "ListItem" => "item",
        "MenuItem" => "menuitem",
        "TabItem" => "tab",
        _ => return None,
    })
}

/// The window in front (the one that gets keys).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WindowInfo {
    pub title: String,
    /// Win32 class name ("#32770" = standard dialog, e.g. the file-open dialog).
    pub class: String,
    /// Address bar of a browser window, as shown (may lack "https://"); None if not a browser or unreadable.
    pub address: Option<String>,
}

impl WindowInfo {
    /// The Windows "open file" dialog a browser shows after "Datei auswählen".
    pub fn is_file_dialog(&self) -> bool {
        let title = self.title.trim().to_lowercase();
        self.class == "#32770"
            || matches!(
                title.as_str(),
                "öffnen" | "open" | "datei hochladen" | "file upload" | "datei öffnen" | "open file"
            )
    }
}

/// UI tree of the OS (Windows UI Automation). Every method may say "unknown" (None / empty):
/// the agent then falls back to safer behaviour (more approval cards), never to fewer.
pub trait UiTree: Send + Sync {
    /// Windows UI Automation element under the point. None on Linux / fake.
    fn element_at(&self, p: PhysPoint) -> Option<UiElement>;
    /// The element with keyboard focus (where keys and typed text go). None = unknown.
    fn focused(&self) -> Option<UiElement> {
        None
    }
    /// Visible elements of the foreground window whose name contains `name` (case-insensitive),
    /// never our own windows. Empty = none or unknown.
    fn find_by_name(&self, _name: &str) -> Vec<(UiElement, PhysRect)> {
        Vec::new()
    }
    /// The foreground window. None = unknown.
    fn foreground(&self) -> Option<WindowInfo> {
        None
    }
    /// Named interactive elements of the page in front (never Pip's own windows), at most `max`, ids 1.. in
    /// document order. Empty = none or unknown (Linux); the agent then finds things in the picture.
    fn outline(&self, _max: usize) -> Vec<OutlineItem> {
        Vec::new()
    }
    /// Scroll this element of the last outline into view. False = could not (the agent turns the wheel).
    fn scroll_into_view(&self, _item: &OutlineItem) -> bool {
        false
    }
    /// The text the field at this point shows (read back after typing). None = unknown or a password field.
    fn value_at(&self, _p: PhysPoint) -> Option<String> {
        None
    }
    /// True if the point is on a window's own title-bar button (close, maximize, minimize, restore).
    /// Pip never clicks those: a popup's ✕ inside the page is fine, the window's ✕ is not.
    fn is_window_control(&self, _p: PhysPoint) -> bool {
        false
    }
    /// Set the text of the field at this point directly (Windows ValuePattern). False = not possible.
    /// Used only to repair a field whose typed text came out wrong; never for password fields.
    fn set_value_at(&self, _p: PhysPoint, _text: &str) -> bool {
        false
    }
    /// Put this path into the "File name" box of the file dialog in front (no typing), read it back and press
    /// Open. False = no file dialog in front, no such box, or the box shows something else (nothing pressed).
    fn set_file_name(&self, _path: &str) -> bool {
        false
    }
    /// The day, month and year parts (role "spinner") of a date input of the last outline, in visual order
    /// (left to right). Each part takes digits only and moves on to the next by itself once full; the year
    /// part takes up to six digits. Empty = no parts or unknown.
    fn date_parts(&self, _item: &OutlineItem) -> Vec<OutlineItem> {
        Vec::new()
    }
    /// Give this element (of the last outline, or a date part) the keyboard focus without a click.
    /// False = could not (the agent clicks instead).
    fn focus_element(&self, _item: &OutlineItem) -> bool {
        false
    }
    /// True if the element at the item's centre is still this item (same name and role): the page did not move
    /// or change under it since the outline was read. True also when the platform cannot tell (no UI tree).
    fn element_matches(&self, _item: &OutlineItem) -> bool {
        true
    }
    /// Press, tick or select the element through the UI tree (Invoke, Toggle or SelectionItem), without the
    /// mouse. False = it has none of these (the agent clicks instead). The caller gates it like a click.
    fn invoke(&self, _item: &OutlineItem) -> bool {
        false
    }
    /// The close button of the browser's "Restore pages" popup ("Seiten wiederherstellen"), if one is shown.
    fn find_restore_popup(&self) -> Option<OutlineItem> {
        None
    }
}

#[derive(Clone)]
pub struct Platform {
    pub screen: Arc<dyn Screen>,
    pub input: Arc<dyn Input>,
    pub ui: Arc<dyn UiTree>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(w: u32, h: u32) -> Frame {
        Frame {
            image: image::RgbaImage::from_pixel(w, h, image::Rgba([255, 255, 255, 255])),
            origin: PhysPoint { x: 0, y: 0 },
            scale: 1.0,
        }
    }

    fn sam(x1: u32, y1: u32, x2: u32, y2: u32, w: u32, h: u32) -> SamBox {
        SamBox { x1, y1, x2, y2, img_w: w, img_h: h }
    }

    #[test]
    fn mask_greys_rect_with_origin_and_clamps() {
        let mut f = frame(100, 50);
        f.origin = PhysPoint { x: 1000, y: 0 };
        f.mask(&[PhysRect { x: 1090, y: 40, w: 50, h: 50 }, PhysRect { x: 0, y: 0, w: 10, h: 10 }]);
        assert_eq!(*f.image.get_pixel(95, 45), MASK_GREY);
        assert_eq!(*f.image.get_pixel(89, 45), image::Rgba([255, 255, 255, 255]));
        assert_eq!(*f.image.get_pixel(0, 0), image::Rgba([255, 255, 255, 255]));
    }

    #[test]
    fn for_api_downscales_long_side_and_maps_back() {
        let mut f = frame(3200, 1800);
        f.origin = PhysPoint { x: -3200, y: 0 };
        let api = f.for_api(1600).unwrap();
        assert_eq!((api.w, api.h), (1600, 900));
        assert!((api.factor - 2.0).abs() < 1e-6);
        let img = image::load_from_memory(&api.png).unwrap();
        assert_eq!((img.width(), img.height()), (1600, 900));
        assert_eq!(api.to_phys(800, 450), PhysPoint { x: -1600, y: 900 });
    }

    #[test]
    fn for_api_keeps_small_frames() {
        let api = frame(800, 600).for_api(1600).unwrap();
        assert_eq!((api.w, api.h, api.factor), (800, 600, 1.0));
        assert_eq!(api.to_phys(10, 20), PhysPoint { x: 10, y: 20 });
    }

    #[test]
    fn box_to_phys_and_sam_rescale() {
        let api = frame(3200, 1800).for_api(1600).unwrap();
        let r = api.box_to_phys(&sam(100, 100, 200, 150, 1600, 900));
        assert_eq!(r, PhysRect { x: 200, y: 200, w: 200, h: 100 });
        // SAM saw a half-size image: its coordinates scale up to the API image first.
        let r2 = api.box_to_phys(&sam(50, 50, 100, 75, 800, 450));
        assert_eq!(r2, r);
        assert!(r.contains(PhysPoint { x: 300, y: 250 }));
    }

    #[test]
    fn crop_png_pads_and_clamps() {
        let api = frame(400, 300).for_api(1600).unwrap();
        let png = api.crop_png(&sam(10, 10, 60, 40, 400, 300), 20).unwrap();
        let img = image::load_from_memory(&png).unwrap();
        assert_eq!((img.width(), img.height()), (80, 60));
        assert!(api.crop_png(&sam(500, 500, 600, 600, 1000, 1000), 0).is_ok());
    }

    #[test]
    fn edge_cases_do_not_panic() {
        // Negative and huge rects, reversed SAM corners, tiny and portrait frames, two-digit labels.
        let mut f = frame(10, 10);
        f.mask(&[PhysRect { x: 5, y: 5, w: -3, h: i32::MAX }, PhysRect { x: -50, y: -50, w: 10, h: 10 }]);
        assert!(frame(0, 0).for_api(1600).is_err());
        let one = frame(1, 1).for_api(0).unwrap();
        assert_eq!((one.w, one.h), (1, 1));
        let tall = frame(1080, 2400).for_api(1600).unwrap();
        assert_eq!((tall.w, tall.h), (720, 1600));
        assert_eq!(tall.to_phys(720, 1600), PhysPoint { x: 1080, y: 2400 });
        let api = frame(300, 200).for_api(1600).unwrap();
        let r = api.box_to_phys(&sam(60, 40, 20, 10, 300, 200));
        assert_eq!(r, PhysRect { x: 20, y: 10, w: 40, h: 30 });
        let boxes: Vec<SamBox> = (0..12).map(|i| sam(i * 20, 150, i * 20 + 30, 200, 300, 200)).collect();
        assert!(api.with_numbered_boxes(&boxes).is_ok());
        assert!(api.with_numbered_boxes(&[sam(290, 190, 300, 200, 0, 0)]).is_ok());
    }

    #[test]
    fn region_diff_and_phys_to_box() {
        let api = frame(400, 300).for_api(1600).unwrap();
        let mut f2 = frame(400, 300);
        f2.mask(&[PhysRect { x: 100, y: 100, w: 100, h: 50 }]);
        let later = f2.for_api(1600).unwrap();
        let b = sam(100, 100, 200, 150, 400, 300);
        assert!(api.region_diff(&later, &b).unwrap() > 100.0, "the box turned grey");
        assert_eq!(api.region_diff(&api, &b), Some(0.0));
        assert!(api.region_diff(&frame(10, 10).for_api(1600).unwrap(), &b).is_none());
        let big = frame(3200, 1800).for_api(1600).unwrap();
        let back = big.phys_to_box(&PhysRect { x: 200, y: 200, w: 200, h: 100 });
        assert_eq!((back.x1, back.y1, back.x2, back.y2), (100, 100, 200, 150));
    }

    #[test]
    fn file_dialog_and_controls() {
        let w = |t: &str, c: &str| WindowInfo { title: t.into(), class: c.into(), address: None };
        assert!(w("Öffnen", "#32770").is_file_dialog());
        assert!(w("Open", "x").is_file_dialog());
        assert!(!w("Evergreen Health – Microsoft Edge", "Chrome_WidgetWin_1").is_file_dialog());
        let e = |c: &str| UiElement { name: "x".into(), control: c.into(), is_password: false };
        assert!(e("Button").is_pressable() && !e("Edit").is_pressable());
        assert!(e("Edit").is_text_input() && !e("Button").is_text_input());
    }

    #[test]
    fn outline_items_map_roles_and_parse_with_defaults() {
        let item: OutlineItem = serde_json::from_str(
            r#"{"id": 3, "role": "edit", "name": "Full name", "rect": {"x": 10, "y": 20, "w": 100, "h": 30}}"#,
        )
        .unwrap();
        assert!(item.enabled && !item.offscreen && !item.is_password && item.value.is_none());
        assert!(item.localized_role.is_none() && !item.is_date && !item.looks_like_date());
        assert_eq!(item.center(), PhysPoint { x: 60, y: 35 });
        assert_eq!(item.as_element().control, "Edit");
        assert!(item.as_element().is_text_input() && item.takes_text());
        assert_eq!(outline_role("SplitButton"), Some("button"));
        assert_eq!(outline_role("Pane"), None);
        for c in ["Edit", "Button", "CheckBox", "RadioButton", "ComboBox", "Hyperlink", "ListItem", "MenuItem"] {
            let role = outline_role(c).unwrap();
            let it = OutlineItem { role: role.into(), ..item.clone() };
            assert_eq!(it.control(), c);
        }
        let part = OutlineItem { role: "spinner".into(), ..item.clone() };
        assert_eq!(part.control(), "Spinner");
        assert!(!part.takes_text());
    }

    #[test]
    fn date_roles() {
        for r in ["Datumsauswahl", "date picker", "Date", "Datums- und Uhrzeitauswahl", "sélecteur de date"] {
            assert!(is_date_role(r), "{r}");
        }
        for r in ["Eingabefeld", "edit", "update", "Schaltfläche", "dated"] {
            assert!(!is_date_role(r), "{r}");
        }
        let mut it: OutlineItem = serde_json::from_str(
            r#"{"id": 1, "role": "edit", "name": "Since", "rect": {"x": 0, "y": 0, "w": 9, "h": 9}, "is_date": true}"#,
        )
        .unwrap();
        assert!(it.looks_like_date());
        it.is_date = false;
        it.localized_role = Some("Datumsauswahl".into());
        assert!(it.looks_like_date());
    }

    #[test]
    fn numbered_boxes_draw_visible_marks() {
        let api = frame(800, 600).for_api(1600).unwrap();
        let png = api.with_numbered_boxes(&[sam(100, 100, 300, 200, 800, 600), sam(0, 0, 50, 40, 800, 600)]).unwrap();
        let img = image::load_from_memory(&png).unwrap().into_rgb8();
        assert_eq!((img.width(), img.height()), (800, 600));
        // Frame line of box 1 is coloured, the box inside stays white.
        assert_eq!(img.get_pixel(200, 99).0, [255, 0, 200]);
        assert_eq!(img.get_pixel(200, 150).0, [255, 255, 255]);
        let changed = img.pixels().filter(|p| p.0 != [255, 255, 255]).count();
        assert!(changed > 2000, "too few marked pixels: {changed}");
    }
}
