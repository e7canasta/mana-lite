//! mana-geometry/src/compact_mask.rs — Compact Mask Storage
//! =============================================================
//! Crop-RLE mask storage backed by the vernier-mask codec.
//! Each mask is column-major RLE scoped to its bounding-box crop,
//! keeping memory O(Σ bbox_area) instead of O(N×H×W).
//!
//! CompactMask is the source-of-truth format for mask transport:
//! a lossless compressed representation from which consumers derive
//! polygons (with their own RDP simplification parameters), dense
//! rasters, areas, or pairwise IoU.

use thiserror::Error;
use vernier_mask::{MaskError, Rle};

#[derive(Debug, Error)]
pub enum CompactMaskError {
    /// Crop buffer length does not match the declared `crop_h * crop_w`.
    #[error("crop shape mismatch: expected {expected} bytes, got {got}")]
    ShapeMismatch { expected: usize, got: usize },
    /// Full-image shapes are inconsistent when accumulating masks.
    #[error("image shape mismatch: expected ({e0},{e1}) got ({g0},{g1})", e0 = expected.0, e1 = expected.1, g0 = got.0, g1 = got.1)]
    ImageShapeMismatch {
        expected: (u32, u32),
        got: (u32, u32),
    },
    /// Bounding-box crop extends beyond the image canvas.
    #[error("bbox [{x1}+{w}, {y1}+{h}] exceeds image ({iw}, {ih})", x1 = offset.0, y1 = offset.1, w = crop_w, h = crop_h, iw = image_shape.1, ih = image_shape.0)]
    BboxOutOfBounds {
        offset: (u32, u32),
        crop_w: u32,
        crop_h: u32,
        image_shape: (u32, u32),
    },
    /// RLE codec error from vernier-mask.
    #[error(transparent)]
    Rle(#[from] MaskError),
    /// Access index out of bounds.
    #[error("index {index} out of bounds (len {len})")]
    IndexOutOfBounds { index: usize, len: usize },
}

/// Run-length-encoded masks stored as crops within bounding boxes.
///
/// # Layout
/// - `rles[i]` is column-major RLE scoped to the bbox crop dimensions.
/// - `offsets[i]` is the `(x1, y1)` pixel position of bbox `i` in the full image.
/// - `image_shape` gives the full image `(height, width)` in pixels.
///
/// # Example
/// ```
/// use mana_geometry::compact_mask::CompactMask;
///
/// let crop: Vec<u8> = [
///     0, 0, 0, 0, 0,
///     0, 1, 1, 0, 0,
///     0, 1, 1, 0, 0,
///     0, 0, 0, 0, 0,
///     0, 0, 0, 0, 0,
/// ].to_vec();
/// let cm = CompactMask::from_dense(&crop, 5, 5, (2, 1), (10, 10)).unwrap();
/// assert_eq!(cm.len(), 1);
/// assert_eq!(cm.area(0).unwrap(), 4);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct CompactMask {
    pub rles: Vec<Rle>,
    pub offsets: Vec<(u32, u32)>,
    pub image_shape: (u32, u32),
}

impl CompactMask {
    /// Build a single-mask `CompactMask` from a dense row-major crop.
    ///
    /// `crop_row_major` has length `crop_h * crop_w`. Foreground is any non-zero
    /// byte (matching vernier-mask's quirk G6).
    ///
    /// `offset` is the `(x1, y1)` pixel position of this bbox in the full image.
    /// `image_shape` is the full image `(height, width)` in pixels.
    pub fn from_dense(
        crop_row_major: &[u8],
        crop_h: u32,
        crop_w: u32,
        offset: (u32, u32),
        image_shape: (u32, u32),
    ) -> Result<Self, CompactMaskError> {
        let expected = (crop_h as usize) * (crop_w as usize);
        if crop_row_major.len() != expected {
            return Err(CompactMaskError::ShapeMismatch {
                expected,
                got: crop_row_major.len(),
            });
        }

        let (off_x, off_y) = offset;
        let (img_h, img_w) = image_shape;
        if off_x + crop_w > img_w || off_y + crop_h > img_h {
            return Err(CompactMaskError::BboxOutOfBounds {
                offset,
                crop_w,
                crop_h,
                image_shape,
            });
        }

        let col_major = row_major_to_col_major(crop_row_major, crop_h, crop_w);
        let rle = Rle::from_raster_bytes(&col_major, crop_h, crop_w)?;

        Ok(Self {
            rles: vec![rle],
            offsets: vec![offset],
            image_shape,
        })
    }

    /// Build a `CompactMask` from a polygon contour (pixel coordinates).
    ///
    /// `polygon` is a flat `[x0, y0, x1, y1, …]` slice in pixel coordinates
    /// relative to the crop. Uses vernier-mask's polygon rasterizer.
    pub fn from_polygon(
        polygon: &[f64],
        crop_h: u32,
        crop_w: u32,
        offset: (u32, u32),
        image_shape: (u32, u32),
    ) -> Result<Self, CompactMaskError> {
        let rle = Rle::from_polygon(polygon, crop_h, crop_w)?;

        Ok(Self {
            rles: vec![rle],
            offsets: vec![offset],
            image_shape,
        })
    }

    /// Build a `CompactMask` from multiple polygons.
    pub fn from_polygons(
        polygons: &[Vec<f64>],
        crop_h: u32,
        crop_w: u32,
        offset: (u32, u32),
        image_shape: (u32, u32),
    ) -> Result<Self, CompactMaskError> {
        let rle = Rle::from_polygons(polygons, crop_h, crop_w)?;

        Ok(Self {
            rles: vec![rle],
            offsets: vec![offset],
            image_shape,
        })
    }

    /// Number of masks in this batch.
    #[inline]
    pub fn len(&self) -> usize {
        self.rles.len()
    }

    /// Whether this batch contains zero masks.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.rles.is_empty()
    }

    /// Foreground pixel count for mask `index`.
    #[inline]
    pub fn area(&self, index: usize) -> Result<u64, CompactMaskError> {
        self.check_index(index)?;
        Ok(self.rles[index].area())
    }

    /// Raw RLE run-length counts for mask `index`.
    ///
    /// Returns the underlying `&[u32]` slice. The first element is always
    /// a background run (per vernier-mask quirk G5); foreground runs sit
    /// at odd indices. Clone the inner `Arc<[u32]>` via `counts.to_vec()`
    /// if you need ownership (e.g. for serialization to `CompactMaskBatchV1`).
    #[inline]
    pub fn rle_counts(&self, index: usize) -> Result<&[u32], CompactMaskError> {
        self.check_index(index)?;
        Ok(&self.rles[index].counts)
    }

    /// Crop height and width for mask `index`.
    #[inline]
    pub fn crop_shape(&self, index: usize) -> Result<(u32, u32), CompactMaskError> {
        self.check_index(index)?;
        Ok((self.rles[index].h, self.rles[index].w))
    }

    /// Encode mask `index` to the CompactMask iceoryx2 wire-format payload.
    ///
    /// Wire format (all native-endian u32):
    /// ```text
    /// [offset_x][offset_y][num_counts][count_0][count_1]...[count_n]
    /// ```
    ///
    /// Total size: 12 + 4 * num_counts bytes. The caller stores this in the
    /// iceoryx2 chunk payload and sets `mask_bytes` in `SegmentV1`.
    pub fn encode_iceoryx2_payload(&self, index: usize) -> Result<Vec<u8>, CompactMaskError> {
        self.check_index(index)?;

        let (off_x, off_y) = self.offsets[index];
        let counts = &self.rles[index].counts;
        let num_counts = counts.len() as u32;

        let mut buf = Vec::with_capacity(12 + num_counts as usize * 4);
        buf.extend_from_slice(&off_x.to_ne_bytes());
        buf.extend_from_slice(&off_y.to_ne_bytes());
        buf.extend_from_slice(&num_counts.to_ne_bytes());
        for &c in counts.iter() {
            buf.extend_from_slice(&c.to_ne_bytes());
        }
        Ok(buf)
    }

    /// Decode a CompactMask iceoryx2 payload into a single-mask `CompactMask`.
    ///
    /// `crop_h` and `crop_w` come from `SegmentV1::mask_height` / `mask_width`.
    /// `image_shape` is the full image `(height, width)` from Frame metadata.
    pub fn from_iceoryx2_payload(
        payload: &[u8],
        crop_h: u32,
        crop_w: u32,
        image_shape: (u32, u32),
    ) -> Result<Self, CompactMaskError> {
        if payload.len() < 12 {
            return Err(CompactMaskError::ShapeMismatch {
                expected: 12,
                got: payload.len(),
            });
        }

        let off_x = u32::from_ne_bytes([payload[0], payload[1], payload[2], payload[3]]);
        let off_y = u32::from_ne_bytes([payload[4], payload[5], payload[6], payload[7]]);
        let num_counts =
            u32::from_ne_bytes([payload[8], payload[9], payload[10], payload[11]]) as usize;

        let expected_len = 12 + num_counts * 4;
        if payload.len() < expected_len {
            return Err(CompactMaskError::ShapeMismatch {
                expected: expected_len,
                got: payload.len(),
            });
        }

        let mut counts = Vec::with_capacity(num_counts);
        for i in 0..num_counts {
            let off = 12 + i * 4;
            let c = u32::from_ne_bytes([
                payload[off],
                payload[off + 1],
                payload[off + 2],
                payload[off + 3],
            ]);
            counts.push(c);
        }

        let rle = Rle::from_counts(crop_h, crop_w, counts);

        Ok(Self {
            rles: vec![rle],
            offsets: vec![(off_x, off_y)],
            image_shape,
        })
    }

    /// Decode mask `index` to a dense full-image row-major buffer.
    ///
    /// Returns a `Vec<u8>` of length `image_shape.0 * image_shape.1` with
    /// foreground pixels set to 1.
    pub fn to_dense(&self, index: usize) -> Result<Vec<u8>, CompactMaskError> {
        self.check_index(index)?;

        let (full_h, full_w) = self.image_shape;
        let full_len = (full_h as usize) * (full_w as usize);
        let mut full = vec![0u8; full_len];

        let col_buf = self.rles[index].to_raster_bytes();
        let (_crop_h, crop_w) = (self.rles[index].h, self.rles[index].w);
        let (off_x, off_y) = self.offsets[index];

        for x in 0..crop_w {
            for y in 0..self.rles[index].h {
                let cm_idx = (x * self.rles[index].h + y) as usize;
                if col_buf[cm_idx] != 0 {
                    let fx = (off_x + x) as usize;
                    let fy = (off_y + y) as usize;
                    if fx < full_w as usize && fy < full_h as usize {
                        full[fy * (full_w as usize) + fx] = 1;
                    }
                }
            }
        }

        Ok(full)
    }

    /// Decode mask `index` to its bbox-scale row-major crop.
    ///
    /// Returns a `Vec<u8>` of length `crop_h * crop_w` without expanding to the
    /// full image. This is O(bbox_area), not O(H×W).
    pub fn decode_crop(&self, index: usize) -> Result<Vec<u8>, CompactMaskError> {
        self.check_index(index)?;

        let col_buf = self.rles[index].to_raster_bytes();
        let crop_h = self.rles[index].h;
        let crop_w = self.rles[index].w;

        Ok(col_major_to_row_major(&col_buf, crop_h, crop_w))
    }

    /// Add another `CompactMask` to this batch.
    ///
    /// `other` must have the same `image_shape`.
    pub fn accumulate(&mut self, other: CompactMask) -> Result<(), CompactMaskError> {
        if other.is_empty() {
            return Ok(());
        }
        if self.image_shape != other.image_shape {
            return Err(CompactMaskError::ImageShapeMismatch {
                expected: self.image_shape,
                got: other.image_shape,
            });
        }
        self.rles.extend(other.rles);
        self.offsets.extend(other.offsets);
        Ok(())
    }
}

impl Extend<CompactMask> for CompactMask {
    fn extend<T: IntoIterator<Item = CompactMask>>(&mut self, iter: T) {
        for other in iter {
            if other.is_empty() {
                continue;
            }
            assert_eq!(
                self.image_shape, other.image_shape,
                "CompactMask Extend: image shape mismatch"
            );
            self.rles.extend(other.rles);
            self.offsets.extend(other.offsets);
        }
    }
}

impl CompactMask {
    /// Resize all masks for a new full-image shape.
    ///
    /// Scales offsets proportionally, decodes each crop, performs nearest-neighbor
    /// resize, and re-encodes. For the clinical monitoring use case (< 5 segments,
    /// 1-2 fps) the decode→resize→encode path is acceptable. The direct RLE
    /// arithmetic path (supervision's `resize`) is not implemented.
    pub fn resize(&self, new_shape: (u32, u32)) -> Result<Self, CompactMaskError> {
        let (old_h, old_w) = self.image_shape;
        let (new_h, new_w) = new_shape;

        if old_h == 0 || old_w == 0 || new_h == 0 || new_w == 0 {
            return Ok(Self {
                rles: Vec::new(),
                offsets: Vec::new(),
                image_shape: new_shape,
            });
        }

        let mut rles = Vec::with_capacity(self.rles.len());
        let mut offsets = Vec::with_capacity(self.offsets.len());

        for (i, rle) in self.rles.iter().enumerate() {
            let (off_x, off_y) = self.offsets[i];
            let crop_h = rle.h;
            let crop_w = rle.w;

            let new_off_x = ((off_x as f64 * new_w as f64) / old_w as f64).round() as u32;
            let new_off_y = ((off_y as f64 * new_h as f64) / old_h as f64).round() as u32;
            let new_crop_w = ((crop_w as f64 * new_w as f64) / old_w as f64).round() as u32;
            let new_crop_h = ((crop_h as f64 * new_h as f64) / old_h as f64).round() as u32;

            let col_buf = rle.to_raster_bytes();
            let row_major = col_major_to_row_major(&col_buf, crop_h, crop_w);
            let resized =
                nearest_neighbor_resize(&row_major, crop_h, crop_w, new_crop_h, new_crop_w);
            let col_major = row_major_to_col_major(&resized, new_crop_h, new_crop_w);

            let new_rle = Rle::from_raster_bytes(&col_major, new_crop_h, new_crop_w)?;
            rles.push(new_rle);
            offsets.push((new_off_x, new_off_y));
        }

        Ok(Self {
            rles,
            offsets,
            image_shape: new_shape,
        })
    }

    /// Re-pack masks: decode each crop, compute the tight bounding box of
    /// non-zero pixels, trim zero-borders, and re-encode.
    ///
    /// Useful after `InferenceSlicer` merge where bboxes are conservative.
    pub fn repack(&self) -> Result<Self, CompactMaskError> {
        let mut rles = Vec::with_capacity(self.rles.len());
        let mut offsets = Vec::with_capacity(self.offsets.len());

        for (i, rle) in self.rles.iter().enumerate() {
            let (off_x, off_y) = self.offsets[i];
            let col_buf = rle.to_raster_bytes();
            let crop_h = rle.h;
            let crop_w = rle.w;

            let tight = tight_bbox_col_major(&col_buf, crop_h, crop_w);
            if tight.2 == 0 || tight.3 == 0 {
                continue;
            }

            let (tx, ty, tw, th) = tight;
            let trimmed = crop_col_major(&col_buf, crop_h, tx, ty, tw, th);
            let new_rle = Rle::from_raster_bytes(&trimmed, th, tw)?;
            rles.push(new_rle);
            offsets.push((off_x + tx, off_y + ty));
        }

        Ok(Self {
            rles,
            offsets,
            image_shape: self.image_shape,
        })
    }

    #[inline]
    fn check_index(&self, index: usize) -> Result<(), CompactMaskError> {
        if index >= self.rles.len() {
            Err(CompactMaskError::IndexOutOfBounds {
                index,
                len: self.rles.len(),
            })
        } else {
            Ok(())
        }
    }

    /// Foreground intersection area of two masks in this batch.
    ///
    /// Computes the intersection of the two crop bounding boxes, then counts
    /// pixels where both masks have foreground. Returns `0` when the bboxes
    /// do not overlap.
    pub fn intersect_area(&self, a: usize, b: usize) -> Result<u64, CompactMaskError> {
        self.check_index(a)?;
        self.check_index(b)?;

        if a == b {
            return self.area(a);
        }

        let (ox_a, oy_a) = self.offsets[a];
        let (ox_b, oy_b) = self.offsets[b];
        let h_a = self.rles[a].h;
        let w_a = self.rles[a].w;
        let h_b = self.rles[b].h;
        let w_b = self.rles[b].w;

        if ox_a == ox_b && oy_a == oy_b && h_a == h_b && w_a == w_b {
            return Ok(self.rles[a].intersect_area(&self.rles[b])?);
        }

        let w_a_i = w_a as i64;
        let h_a_i = h_a as i64;
        let w_b_i = w_b as i64;
        let h_b_i = h_b as i64;

        let ix1 = (ox_a as i64).max(ox_b as i64);
        let iy1 = (oy_a as i64).max(oy_b as i64);
        let ix2 = ((ox_a as i64 + w_a_i).min(ox_b as i64 + w_b_i)).max(ix1);
        let iy2 = ((oy_a as i64 + h_a_i).min(oy_b as i64 + h_b_i)).max(iy1);
        let iw = (ix2 - ix1) as usize;
        let ih = (iy2 - iy1) as usize;
        if iw == 0 || ih == 0 {
            return Ok(0);
        }

        let crop_a = self.decode_crop(a)?;
        let crop_b = self.decode_crop(b)?;

        let w_a_u = w_a as usize;
        let w_b_u = w_b as usize;
        let ox_a_i = ox_a as i64;
        let oy_a_i = oy_a as i64;
        let ox_b_i = ox_b as i64;
        let oy_b_i = oy_b as i64;

        let mut inter = 0u64;
        for dy in 0..ih {
            let ay = (iy1 - oy_a_i + dy as i64) as usize;
            let by = (iy1 - oy_b_i + dy as i64) as usize;
            let row_a_off = ay * w_a_u;
            let row_b_off = by * w_b_u;
            for dx in 0..iw {
                let ax = (ix1 - ox_a_i + dx as i64) as usize;
                let bx = (ix1 - ox_b_i + dx as i64) as usize;
                if crop_a[row_a_off + ax] != 0 && crop_b[row_b_off + bx] != 0 {
                    inter += 1;
                }
            }
        }
        Ok(inter)
    }

    /// Intersection-over-union of two masks in this batch.
    ///
    /// Returns `0.0` when both masks are empty.
    pub fn iou(&self, a: usize, b: usize) -> Result<f64, CompactMaskError> {
        let inter = self.intersect_area(a, b)?;
        let area_a = self.area(a)?;
        let area_b = self.area(b)?;
        let union = area_a + area_b - inter;
        if union == 0 {
            Ok(0.0)
        } else {
            Ok(inter as f64 / union as f64)
        }
    }
}

// ─── Column-major ↔ Row-major conversion utilities ────────────

#[inline]
fn row_major_to_col_major(row: &[u8], h: u32, w: u32) -> Vec<u8> {
    let (h, w) = (h as usize, w as usize);
    let len = h * w;
    let mut col = vec![0u8; len];
    for y in 0..h {
        let row_off = y * w;
        for x in 0..w {
            col[x * h + y] = row[row_off + x];
        }
    }
    col
}

#[inline]
fn col_major_to_row_major(col: &[u8], h: u32, w: u32) -> Vec<u8> {
    let (h, w) = (h as usize, w as usize);
    let len = h * w;
    let mut row = vec![0u8; len];
    for y in 0..h {
        let row_off = y * w;
        for x in 0..w {
            row[row_off + x] = col[x * h + y];
        }
    }
    row
}

/// Nearest-neighbor resize of a row-major binary image.
fn nearest_neighbor_resize(src: &[u8], src_h: u32, src_w: u32, dst_h: u32, dst_w: u32) -> Vec<u8> {
    let (src_h, src_w) = (src_h as usize, src_w as usize);
    let (dst_h, dst_w) = (dst_h as usize, dst_w as usize);
    let mut dst = vec![0u8; dst_w * dst_h];

    for dy in 0..dst_h {
        let sy = (dy * src_h) / dst_h;
        let row_off = sy * src_w;
        let dst_row_off = dy * dst_w;
        for dx in 0..dst_w {
            let sx = (dx * src_w) / dst_w;
            dst[dst_row_off + dx] = src[row_off + sx];
        }
    }
    dst
}

/// Compute the tight bounding box of non-zero pixels in a column-major buffer.
///
/// Returns `(x, y, w, h)` or `(0, 0, 0, 0)` if no foreground pixels exist.
fn tight_bbox_col_major(col: &[u8], h: u32, w: u32) -> (u32, u32, u32, u32) {
    let (h, w) = (h as usize, w as usize);
    let mut min_x = w;
    let mut min_y = h;
    let mut max_x: usize = 0;
    let mut max_y: usize = 0;
    let mut found = false;

    for x in 0..w {
        for y in 0..h {
            if col[x * h + y] != 0 {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
                found = true;
            }
        }
    }

    if !found {
        return (0, 0, 0, 0);
    }

    (
        min_x as u32,
        min_y as u32,
        (max_x - min_x + 1) as u32,
        (max_y - min_y + 1) as u32,
    )
}

/// Crop a region from a column-major buffer.
///
/// Returns a new column-major buffer of size `th * tw` containing the region
/// starting at `(tx, ty)`.
fn crop_col_major(col: &[u8], full_h: u32, tx: u32, ty: u32, tw: u32, th: u32) -> Vec<u8> {
    let (full_h, tx, ty, tw, th) = (
        full_h as usize,
        tx as usize,
        ty as usize,
        tw as usize,
        th as usize,
    );
    let mut cropped = vec![0u8; tw * th];

    for x in 0..tw {
        for y in 0..th {
            let src_idx = (tx + x) * full_h + (ty + y);
            let dst_idx = x * th + y;
            cropped[dst_idx] = col[src_idx];
        }
    }
    cropped
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── from_dense / to_dense round-trip ──────────────────────

    #[test]
    fn from_dense_to_dense_roundtrip() {
        // 10×10 image, 3×3 white square at (4,4) → bbox (4,4,3,3)
        let mut full = vec![0u8; 10 * 10];
        for y in 4..7 {
            for x in 4..7 {
                full[y * 10 + x] = 1;
            }
        }

        let cm = CompactMask::from_dense(&full, 10, 10, (0, 0), (10, 10)).unwrap();
        assert_eq!(cm.len(), 1);
        assert_eq!(cm.area(0).unwrap(), 9);

        let decoded = cm.to_dense(0).unwrap();
        assert_eq!(decoded.len(), 100);
        assert_eq!(
            &decoded, &full,
            "decoded must match original pixel-for-pixel"
        );
    }

    #[test]
    fn from_dense_to_dense_roundtrip_with_offset() {
        // 20×20 image, 4×4 white square at (10, 5), row-major
        let mut full = vec![0u8; 20 * 20];
        for y in 5..9 {
            for x in 10..14 {
                full[y * 20 + x] = 1;
            }
        }

        let cm = CompactMask::from_dense(&full, 20, 20, (0, 0), (20, 20)).unwrap();
        assert_eq!(cm.area(0).unwrap(), 16);

        let decoded = cm.to_dense(0).unwrap();
        assert_eq!(decoded.len(), 400);
        assert_eq!(&decoded, &full, "decoded must match original");
    }

    // ─── Empty mask ────────────────────────────────────────────

    #[test]
    fn empty_mask_zero_area() {
        let full = vec![0u8; 10 * 10];
        let cm = CompactMask::from_dense(&full, 10, 10, (0, 0), (10, 10)).unwrap();
        assert_eq!(cm.area(0).unwrap(), 0);

        let decoded = cm.to_dense(0).unwrap();
        assert_eq!(decoded.iter().sum::<u8>(), 0);
    }

    #[test]
    fn is_empty_after_new() {
        let cm = CompactMask {
            rles: Vec::new(),
            offsets: Vec::new(),
            image_shape: (10, 10),
        };
        assert!(cm.is_empty());
        assert_eq!(cm.len(), 0);
    }

    // ─── Multiple masks ────────────────────────────────────────

    #[test]
    fn multiple_masks_accumulate() {
        // Mask A: 2×2 square at (0, 0)
        let mut mask_a = vec![0u8; 10 * 10];
        mask_a[0] = 1;
        mask_a[1] = 1;
        mask_a[10] = 1;
        mask_a[11] = 1;
        let cm_a = CompactMask::from_dense(&mask_a, 10, 10, (0, 0), (10, 10)).unwrap();
        assert_eq!(cm_a.area(0).unwrap(), 4);

        // Mask B: 3×1 bar at (5, 7)
        let mut mask_b = vec![0u8; 10 * 10];
        mask_b[7 * 10 + 5] = 1;
        mask_b[7 * 10 + 6] = 1;
        mask_b[7 * 10 + 7] = 1;
        let cm_b = CompactMask::from_dense(&mask_b, 10, 10, (0, 0), (10, 10)).unwrap();
        assert_eq!(cm_b.area(0).unwrap(), 3);

        let mut batch = cm_a.clone();
        batch.accumulate(cm_b).unwrap();
        assert_eq!(batch.len(), 2);
        assert_eq!(batch.area(0).unwrap(), 4);
        assert_eq!(batch.area(1).unwrap(), 3);
    }

    #[test]
    fn extend_collects_multiple() {
        let cm_a = CompactMask::from_dense(&[1u8; 4], 2, 2, (0, 0), (10, 10)).unwrap();
        let cm_b = CompactMask::from_dense(&[1u8; 4], 2, 2, (0, 0), (10, 10)).unwrap();
        let cm_c = CompactMask::from_dense(&[1u8; 4], 2, 2, (0, 0), (10, 10)).unwrap();

        let mut batch = cm_a.clone();
        batch.extend([cm_b, cm_c]);
        assert_eq!(batch.len(), 3);
        assert_eq!(batch.area(0).unwrap(), 4);
        assert_eq!(batch.area(1).unwrap(), 4);
        assert_eq!(batch.area(2).unwrap(), 4);
    }

    #[test]
    fn accumulate_shape_mismatch_rejected() {
        let full = vec![0u8; 10 * 10];
        let cm_a = CompactMask::from_dense(&full, 10, 10, (0, 0), (10, 10)).unwrap();
        let cm_b = CompactMask::from_dense(&full, 10, 10, (0, 0), (20, 20)).unwrap();

        let mut batch = cm_a;
        let err = batch.accumulate(cm_b).unwrap_err();
        assert!(matches!(err, CompactMaskError::ImageShapeMismatch { .. }));
    }

    #[test]
    fn accumulate_empty_is_noop() {
        let full = vec![0u8; 10 * 10];
        let mut cm = CompactMask::from_dense(&full, 10, 10, (0, 0), (10, 10)).unwrap();
        let empty = CompactMask {
            rles: Vec::new(),
            offsets: Vec::new(),
            image_shape: (10, 10),
        };

        let before = cm.len();
        cm.accumulate(empty).unwrap();
        assert_eq!(cm.len(), before);
    }

    // ─── Crop (bbox-scale only) ────────────────────────────────

    #[test]
    fn crop_smaller_than_full() {
        let mut full = vec![0u8; 20 * 20];
        for y in 8..12 {
            for x in 5..10 {
                full[y * 20 + x] = 1;
            }
        }

        let cm = CompactMask::from_dense(&full, 20, 20, (0, 0), (20, 20)).unwrap();
        let crop = cm.decode_crop(0).unwrap();

        // Crop is just the RLE decoded buffer (column-major→row-major),
        // still full-image sized in our construction since offset is (0,0)
        // and the crop dimensions match the RLE shape.
        assert_eq!(crop.len(), 400);
        assert_eq!(crop.iter().sum::<u8>(), 4 * 5); // 20 fg pixels
    }

    #[test]
    fn crop_offset_preserves_placement() {
        // Mask is a 3×2 bar with offset (5, 8) in 20×20 canvas
        let crop_w = 3u32;
        let crop_h = 2u32;
        let mut crop_data = vec![0u8; (crop_w * crop_h) as usize];
        crop_data[0] = 1;
        crop_data[1] = 1;
        crop_data[3] = 1;
        crop_data[4] = 1;

        let cm = CompactMask::from_dense(&crop_data, crop_h, crop_w, (5, 8), (20, 20)).unwrap();
        let dense = cm.to_dense(0).unwrap();

        // Foreground should be at positions (5,8), (6,8), (5,9), (6,9)
        assert_eq!(dense[8 * 20 + 5], 1);
        assert_eq!(dense[8 * 20 + 6], 1);
        assert_eq!(dense[9 * 20 + 5], 1);
        assert_eq!(dense[9 * 20 + 6], 1);
        assert_eq!(dense.iter().sum::<u8>(), 4);
    }

    // ─── Bbox at origin ────────────────────────────────────────

    #[test]
    fn bbox_at_origin_no_shift() {
        let crop = vec![1u8; 4]; // 2×2 square, all fg
        let cm = CompactMask::from_dense(&crop, 2, 2, (0, 0), (10, 10)).unwrap();
        assert_eq!(cm.offsets[0], (0, 0));

        let dense = cm.to_dense(0).unwrap();
        assert_eq!(dense[0], 1);
        assert_eq!(dense[1], 1);
        assert_eq!(dense[10], 1);
        assert_eq!(dense[11], 1);
    }

    // ─── Area matches pixel count ──────────────────────────────

    #[test]
    fn area_matches_pixel_count() {
        let mut full = vec![0u8; 15 * 15];
        for y in 2..5 {
            for x in 3..8 {
                full[y * 15 + x] = 1;
            }
        }
        // 3 rows × 5 cols = 15 pixels
        let cm = CompactMask::from_dense(&full, 15, 15, (0, 0), (15, 15)).unwrap();
        assert_eq!(cm.area(0).unwrap(), 15);
    }

    // ─── Index out of bounds ───────────────────────────────────

    #[test]
    fn index_out_of_bounds_errors() {
        let cm = CompactMask {
            rles: Vec::new(),
            offsets: Vec::new(),
            image_shape: (10, 10),
        };
        let err = cm.area(0).unwrap_err();
        assert!(matches!(err, CompactMaskError::IndexOutOfBounds { .. }));
    }

    #[test]
    fn shape_mismatch_detected() {
        let crop = vec![0u8; 5]; // should be 4 for 2×2
        let err = CompactMask::from_dense(&crop, 2, 2, (0, 0), (10, 10)).unwrap_err();
        assert!(matches!(err, CompactMaskError::ShapeMismatch { .. }));
    }

    #[test]
    fn bbox_out_of_bounds_rejected() {
        let crop = vec![1u8; 100]; // 10×10 crop placed at (95, 90) in 100×100 canvas
        let err = CompactMask::from_dense(&crop, 10, 10, (95, 5), (100, 100)).unwrap_err();
        assert!(matches!(err, CompactMaskError::BboxOutOfBounds { .. }));
    }

    #[test]
    fn bbox_out_of_bounds_height() {
        let crop = vec![1u8; 100];
        let err = CompactMask::from_dense(&crop, 10, 10, (5, 95), (100, 100)).unwrap_err();
        assert!(matches!(err, CompactMaskError::BboxOutOfBounds { .. }));
    }

    // ─── Non-zero byte binarization (quirk G6) ─────────────────

    #[test]
    fn nonzero_bytes_are_foreground() {
        // Mixed values: 0 bg, 2 fg, 255 fg, 0 bg
        let crop = vec![0u8, 2, 255, 0]; // 2×2
        let cm = CompactMask::from_dense(&crop, 2, 2, (0, 0), (4, 4)).unwrap();
        assert_eq!(cm.area(0).unwrap(), 2);

        let dense = cm.to_dense(0).unwrap();
        // After binarization, both 2 and 255 become 1
        let fg_count = dense.iter().filter(|&&b| b == 1).count();
        assert_eq!(fg_count, 2);
    }

    // ─── Resize ────────────────────────────────────────────────

    #[test]
    fn resize_doubles_everything() {
        // 4×4 crop at (2,2) in 8×8 canvas → resize to 16×16 canvas
        let crop = vec![1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]; // 4×4, 2×2 fg square
        let cm = CompactMask::from_dense(&crop, 4, 4, (2, 2), (8, 8)).unwrap();

        let resized = cm.resize((16, 16)).unwrap();
        // Offset doubles: (2*16/8, 2*16/8) = (4, 4)
        assert_eq!(resized.offsets[0], (4, 4));
        // Crop dimensions double: (4*16/8, 4*16/8) = (8, 8)
        assert_eq!(resized.rles[0].h, 8);
        assert_eq!(resized.rles[0].w, 8);
        // Fg area quadruples: 4 → 16
        assert_eq!(resized.area(0).unwrap(), 16);
    }

    // ─── Repack ────────────────────────────────────────────────

    #[test]
    fn repack_trims_zero_borders() {
        // 8×8 crop with 3×3 fg inside, loose bbox
        let mut crop = vec![0u8; 8 * 8];
        for y in 3..6 {
            for x in 2..5 {
                crop[y * 8 + x] = 1;
            }
        }
        let cm = CompactMask::from_dense(&crop, 8, 8, (10, 10), (30, 30)).unwrap();
        assert_eq!(cm.rles[0].h, 8);
        assert_eq!(cm.rles[0].w, 8);

        let repacked = cm.repack().unwrap();
        // Tight bbox should be 3×3
        assert_eq!(repacked.rles[0].h, 3);
        assert_eq!(repacked.rles[0].w, 3);
        // Offset shifts by (2, 3) — the zero border
        assert_eq!(repacked.offsets[0], (12, 13));
        assert_eq!(repacked.area(0).unwrap(), 9);
    }

    #[test]
    fn repack_all_background_skipped() {
        let crop = vec![0u8; 4 * 4];
        let cm = CompactMask::from_dense(&crop, 4, 4, (0, 0), (10, 10)).unwrap();
        let repacked = cm.repack().unwrap();
        assert!(repacked.is_empty(), "all-bg masks should be dropped");
    }

    // ─── Payload encode / decode (iceoryx2 wire) ────────────────

    #[test]
    fn payload_roundtrip_single_mask() {
        let mut full = vec![0u8; 20 * 20];
        for y in 5..10 {
            for x in 8..14 {
                full[y * 20 + x] = 1;
            }
        }
        let cm = CompactMask::from_dense(&full, 20, 20, (0, 0), (20, 20)).unwrap();

        let payload = cm.encode_iceoryx2_payload(0).unwrap();
        assert!(payload.len() >= 12, "payload too small: {}", payload.len());

        let (crop_h, crop_w) = cm.crop_shape(0).unwrap();
        let decoded =
            CompactMask::from_iceoryx2_payload(&payload, crop_h, crop_w, (20, 20)).unwrap();

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded.area(0).unwrap(), cm.area(0).unwrap());
        assert_eq!(decoded.offsets[0], cm.offsets[0]);

        let dense_orig = cm.to_dense(0).unwrap();
        let dense_dec = decoded.to_dense(0).unwrap();
        assert_eq!(&dense_dec, &dense_orig);
    }

    #[test]
    fn payload_roundtrip_with_offset() {
        let crop = vec![1u8; 9]; // 3×3 square
        let cm = CompactMask::from_dense(&crop, 3, 3, (7, 4), (30, 30)).unwrap();

        let payload = cm.encode_iceoryx2_payload(0).unwrap();
        let (crop_h, crop_w) = cm.crop_shape(0).unwrap();
        let decoded =
            CompactMask::from_iceoryx2_payload(&payload, crop_h, crop_w, (30, 30)).unwrap();

        assert_eq!(decoded.offsets[0], (7, 4));
        assert_eq!(decoded.area(0).unwrap(), 9);
    }

    #[test]
    fn payload_truncated_rejected() {
        let err = CompactMask::from_iceoryx2_payload(&[0u8; 8], 10, 10, (20, 20)).unwrap_err();
        assert!(matches!(err, CompactMaskError::ShapeMismatch { .. }));
    }

    #[test]
    fn payload_counts_len_mismatch_rejected() {
        // Claim 3 counts but provide only 2
        let mut buf = Vec::new();
        buf.extend_from_slice(&0u32.to_ne_bytes()); // off_x
        buf.extend_from_slice(&0u32.to_ne_bytes()); // off_y
        buf.extend_from_slice(&3u32.to_ne_bytes()); // num_counts = 3
        buf.extend_from_slice(&10u32.to_ne_bytes()); // only 1 count
        buf.extend_from_slice(&20u32.to_ne_bytes()); // only 2 counts

        let err = CompactMask::from_iceoryx2_payload(&buf, 10, 10, (20, 20)).unwrap_err();
        assert!(matches!(err, CompactMaskError::ShapeMismatch { .. }));
    }

    #[test]
    fn payload_multiple_masks_roundtrip() {
        let mut mask_a = vec![0u8; 10 * 10];
        mask_a[0] = 1;
        mask_a[1] = 1;
        mask_a[10] = 1;
        mask_a[11] = 1;
        let cm_a = CompactMask::from_dense(&mask_a, 10, 10, (0, 0), (10, 10)).unwrap();

        let mut mask_b = vec![0u8; 10 * 10];
        mask_b[3 * 10 + 5] = 1;
        let cm_b = CompactMask::from_dense(&mask_b, 10, 10, (0, 0), (10, 10)).unwrap();

        // Encode each separately (as they would be packed into iceoryx2 chunk)
        let payload_a = cm_a.encode_iceoryx2_payload(0).unwrap();
        let payload_b = cm_b.encode_iceoryx2_payload(0).unwrap();

        let (h_a, w_a) = cm_a.crop_shape(0).unwrap();
        let (h_b, w_b) = cm_b.crop_shape(0).unwrap();

        let mut batch = CompactMask::from_iceoryx2_payload(&payload_a, h_a, w_a, (10, 10)).unwrap();
        let dec_b = CompactMask::from_iceoryx2_payload(&payload_b, h_b, w_b, (10, 10)).unwrap();
        batch.accumulate(dec_b).unwrap();

        assert_eq!(batch.len(), 2);
        assert_eq!(batch.area(0).unwrap(), 4);
        assert_eq!(batch.area(1).unwrap(), 1);
    }

    // ─── intersect_area / iou ────────────────────────────────────

    #[test]
    fn intersect_area_non_overlapping_is_zero() {
        let cm_a = CompactMask::from_dense(&[1u8; 4], 2, 2, (0, 0), (10, 10)).unwrap();
        let mut batch = cm_a.clone();
        let cm_b = CompactMask::from_dense(&[1u8; 4], 2, 2, (8, 8), (10, 10)).unwrap();
        batch.accumulate(cm_b).unwrap();

        assert_eq!(batch.intersect_area(0, 1).unwrap(), 0);
    }

    #[test]
    fn intersect_area_full_overlap() {
        let cm_a = CompactMask::from_dense(&[1u8; 4], 2, 2, (0, 0), (10, 10)).unwrap();
        let mut batch = cm_a.clone();
        let cm_b = CompactMask::from_dense(&[1u8; 4], 2, 2, (0, 0), (10, 10)).unwrap();
        batch.accumulate(cm_b).unwrap();

        assert_eq!(batch.intersect_area(0, 1).unwrap(), 4);
    }

    #[test]
    fn intersect_area_partial_overlap() {
        let mut full = vec![0u8; 10 * 10];
        for y in 0..4 {
            for x in 0..4 {
                full[y * 10 + x] = 1;
            }
        }
        let cm = CompactMask::from_dense(&full, 10, 10, (0, 0), (10, 10)).unwrap();

        // Mask B: 3×3 square at (2, 2) — overlaps 2×2 with A (pixels (2,2)-(3,3))
        let mut crop_b = vec![0u8; 3 * 3];
        for y in 0..3 {
            for x in 0..3 {
                crop_b[y * 3 + x] = 1;
            }
        }
        let mut batch = cm.clone();
        let cm_b = CompactMask::from_dense(&crop_b, 3, 3, (2, 2), (10, 10)).unwrap();
        batch.accumulate(cm_b).unwrap();

        assert_eq!(batch.intersect_area(0, 1).unwrap(), 4);
    }

    #[test]
    fn intersect_area_same_mask() {
        let cm = CompactMask::from_dense(&[1u8; 4], 2, 2, (0, 0), (10, 10)).unwrap();
        assert_eq!(cm.intersect_area(0, 0).unwrap(), 4);
    }

    #[test]
    fn iou_perfect_overlap() {
        let cm_a = CompactMask::from_dense(&[1u8; 4], 2, 2, (0, 0), (10, 10)).unwrap();
        let mut batch = cm_a.clone();
        let cm_b = CompactMask::from_dense(&[1u8; 4], 2, 2, (0, 0), (10, 10)).unwrap();
        batch.accumulate(cm_b).unwrap();

        let i = batch.iou(0, 1).unwrap();
        assert!((i - 1.0).abs() < 1e-9, "IoU should be 1.0, got {i}");
    }

    #[test]
    fn iou_non_overlapping_is_zero() {
        let cm_a = CompactMask::from_dense(&[1u8; 4], 2, 2, (0, 0), (10, 10)).unwrap();
        let mut batch = cm_a.clone();
        let cm_b = CompactMask::from_dense(&[1u8; 4], 2, 2, (8, 8), (10, 10)).unwrap();
        batch.accumulate(cm_b).unwrap();

        let i = batch.iou(0, 1).unwrap();
        assert!((i - 0.0).abs() < 1e-9, "IoU should be 0.0, got {i}");
    }

    #[test]
    fn iou_empty_masks() {
        let cm_a = CompactMask::from_dense(&[0u8; 4], 2, 2, (0, 0), (10, 10)).unwrap();
        let mut batch = cm_a.clone();
        let cm_b = CompactMask::from_dense(&[0u8; 4], 2, 2, (0, 0), (10, 10)).unwrap();
        batch.accumulate(cm_b).unwrap();

        assert_eq!(batch.iou(0, 1).unwrap(), 0.0);
    }
}
