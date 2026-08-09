//! Mask overlay / polygon / debug raster helpers for Rerun viz.

use image::{Rgb, RgbImage};
use imageproc::drawing::draw_line_segment_mut;
use mana_geometry::compact_mask::CompactMask;

use crate::detection::{CropRect, Detection};

use super::PALETTE;

pub(super) fn polygon_in_roi(poly: &[[f32; 2]], fw: f32, fh: f32, roi: CropRect) -> Vec<[f32; 2]> {
    let roi_x1 = roi.x1 as f32;
    let roi_y1 = roi.y1 as f32;
    let roi_x2 = roi.x2 as f32;
    let roi_y2 = roi.y2 as f32;
    let global: Vec<[f32; 2]> = poly
        .iter()
        .map(|point| [point[0] * fw, point[1] * fh])
        .collect();
    let min_x = global.iter().map(|point| point[0]).reduce(f32::min);
    let max_x = global.iter().map(|point| point[0]).reduce(f32::max);
    let min_y = global.iter().map(|point| point[1]).reduce(f32::min);
    let max_y = global.iter().map(|point| point[1]).reduce(f32::max);
    if !matches!((min_x, max_x, min_y, max_y), (Some(min_x), Some(max_x), Some(min_y), Some(max_y))
        if max_x >= roi_x1 && min_x <= roi_x2 && max_y >= roi_y1 && min_y <= roi_y2)
    {
        return Vec::new();
    }

    let mut local: Vec<[f32; 2]> = global
        .into_iter()
        .map(|[x, y]| {
            [
                (x - roi_x1).clamp(0.0, roi_x2 - roi_x1),
                (y - roi_y1).clamp(0.0, roi_y2 - roi_y1),
            ]
        })
        .collect();
    if let Some(first) = local.first().copied() {
        local.push(first);
    }
    local
}

/// Frame-pixel vertices of a frame-normalized contour, closed by repeating
/// the first vertex so Rerun renders a full polygon outline.
pub(super) fn frame_strip(poly: &[[f32; 2]], fw: f32, fh: f32) -> Vec<[f32; 2]> {
    let mut strip: Vec<[f32; 2]> = poly.iter().map(|v| [v[0] * fw, v[1] * fh]).collect();
    if let Some(first) = strip.first().copied() {
        strip.push(first);
    }
    strip
}

fn mask_space_points(
    poly: &[[f32; 2]],
    fw: f32,
    fh: f32,
    origin: [u32; 2],
    mask_w: u32,
    mask_h: u32,
) -> Vec<(f32, f32)> {
    poly.iter()
        .map(|v| {
            let mx = ((v[0] * fw - origin[0] as f32) / mask_w as f32).clamp(0.0, 1.0);
            let my = ((v[1] * fh - origin[1] as f32) / mask_h as f32).clamp(0.0, 1.0);
            (mx * mask_w as f32, my * mask_h as f32)
        })
        .collect()
}

fn draw_overlay_line(buf: &mut [u8], mask_w: u32, mask_h: u32, a: (f32, f32), b: (f32, f32)) {
    let dist = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
    let steps = dist.ceil().max(1.0) as u32;
    for s in 0..=steps {
        let t = s as f32 / steps as f32;
        for dx in -1..=1i32 {
            for dy in -1..=1i32 {
                let x = (a.0 + (b.0 - a.0) * t + dx as f32).round() as i64;
                let y = (a.1 + (b.1 - a.1) * t + dy as f32).round() as i64;
                if x >= 0 && y >= 0 && x < mask_w as i64 && y < mask_h as i64 {
                    buf[(y as usize) * (mask_w as usize) + x as usize] = 0xFF;
                }
            }
        }
    }
}

/// Class-id buffer in mask space: 0 = background, 1..8 = instance id,
/// `0xFF` = polygon contour drawn on top of the mask fill.
pub(super) fn build_mask_overlay(
    masked: &[&Detection],
    mask_w: u32,
    mask_h: u32,
    frame_w: u32,
    frame_h: u32,
) -> Vec<u8> {
    let mut overlay = vec![0u8; (mask_w * mask_h) as usize];
    let fw = frame_w.max(1) as f32;
    let fh = frame_h.max(1) as f32;

    for (index, detection) in masked.iter().enumerate() {
        let Some(mask) = &detection.mask else {
            continue;
        };
        let Ok(raster) = mask.compact.decode_crop(0) else {
            continue;
        };
        let Some(rle) = mask.compact.rles.first() else {
            continue;
        };
        let (off_x, off_y) = mask.compact.offsets.first().copied().unwrap_or((0, 0));
        let (bbox_w, bbox_h) = (rle.w as usize, rle.h as usize);
        let class_id = ((index % 7) + 1) as u8;
        for row in 0..bbox_h {
            for col in 0..bbox_w {
                if raster[row * bbox_w + col] != 0 {
                    let x = off_x as usize + col;
                    let y = off_y as usize + row;
                    if x < mask_w as usize && y < mask_h as usize {
                        overlay[y * mask_w as usize + x] = class_id;
                    }
                }
            }
        }

        for poly in mask.polygons.as_ref() {
            if poly.len() < 2 {
                continue;
            }
            let pts = mask_space_points(poly, fw, fh, mask.origin, mask_w, mask_h);
            for i in 0..pts.len() {
                draw_overlay_line(
                    &mut overlay,
                    mask_w,
                    mask_h,
                    pts[i],
                    pts[(i + 1) % pts.len()],
                );
            }
        }
    }
    overlay
}

fn draw_thick_line(img: &mut RgbImage, a: (f32, f32), b: (f32, f32), color: Rgb<u8>) {
    for dx in -1..=1i32 {
        for dy in -1..=1i32 {
            draw_line_segment_mut(
                img,
                (a.0 + dx as f32, a.1 + dy as f32),
                (b.0 + dx as f32, b.1 + dy as f32),
                color,
            );
        }
    }
}

pub(super) fn render_mask_debug_images(
    masked: &[&Detection],
    frame_w: u32,
    frame_h: u32,
) -> Option<(RgbImage, RgbImage, u32, u32)> {
    let [mask_w, mask_h] = masked
        .first()
        .and_then(|d| d.mask.as_ref())
        .map(|m| m.mask_dims)?;
    if mask_w == 0 || mask_h == 0 {
        return None;
    }

    let mut mask_img = RgbImage::new(mask_w, mask_h);
    let mut poly_img = RgbImage::new(mask_w, mask_h);
    let fw = frame_w.max(1) as f32;
    let fh = frame_h.max(1) as f32;

    for (index, detection) in masked.iter().enumerate() {
        let Some(mask) = &detection.mask else {
            continue;
        };
        let color = Rgb(PALETTE[index % PALETTE.len()]);

        if let Ok(raster) = mask.compact.decode_crop(0) {
            let Some(rle) = mask.compact.rles.first() else {
                continue;
            };
            let (off_x, off_y) = mask.compact.offsets.first().copied().unwrap_or((0, 0));
            let (bbox_w, bbox_h) = (rle.w, rle.h);
            for row in 0..bbox_h {
                for col in 0..bbox_w {
                    if raster[(row * bbox_w + col) as usize] != 0 {
                        let x = off_x as u32 + col;
                        let y = off_y as u32 + row;
                        if x < mask_w && y < mask_h {
                            mask_img.put_pixel(x, y, color);
                        }
                    }
                }
            }
        }

        for poly in mask.polygons.as_ref() {
            if poly.len() < 2 {
                continue;
            }
            let pts = mask_space_points(poly, fw, fh, mask.origin, mask_w, mask_h);
            for i in 0..pts.len() {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                draw_thick_line(&mut poly_img, a, b, color);
                draw_thick_line(&mut mask_img, a, b, Rgb([255, 255, 255]));
            }
        }
    }

    Some((mask_img, poly_img, mask_w, mask_h))
}
