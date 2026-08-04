//! mana-video/src/format.rs — Pixel format conversion utilities
//! ===============================================================
//! Canonical implementation of `parse_pixel_format` and `pack_frame_into`.
//! Single source of truth — imported by `mana-rtsp` and `mana-ingest`.

use anyhow::Result;

use crate::PixelFormat;

/// Convert a pixel format name from TOML config into both the Mana enum and
/// the corresponding ffmpeg Pixel. Single source of truth — avoid divergent
/// match arms in FileSource, RtspSource, and the rtsp background thread.
///
/// Accepts canonical names (`RGB8`, `BGR8`, `NV12`) and common aliases
/// (`rgb24`, `bgr24`, `nv12`) case-insensitively.
pub fn parse_pixel_format(name: &str) -> Result<(PixelFormat, ffmpeg_next::format::Pixel)> {
    match name.trim().to_ascii_uppercase().as_str() {
        "RGB8" | "RGB24" => Ok((PixelFormat::Rgb8, ffmpeg_next::format::Pixel::RGB24)),
        "BGR8" | "BGR24" => Ok((PixelFormat::Bgr8, ffmpeg_next::format::Pixel::BGR24)),
        "NV12" => Ok((PixelFormat::Nv12, ffmpeg_next::format::Pixel::NV12)),
        other => anyhow::bail!(
            "unsupported pixel format: {other} (supported: RGB8, BGR8, NV12; aliases: rgb24, bgr24)"
        ),
    }
}

/// Copy a decoded ffmpeg frame into `dst` as tightly-packed pixels.
///
/// ffmpeg allocates each plane with a `linesize` (stride) padded for SIMD
/// alignment, so `frame.data(p)` carries trailing bytes per row. Copying the
/// raw plane buffer would shear the image on any resolution whose row size is
/// not already stride-aligned. This walks each plane row-by-row, copying only
/// the meaningful `tight_row` bytes.
///
/// Semi-planar NV12 is handled explicitly (full-res Y plane + interleaved UV
/// plane at 2x2 chroma subsampling); the packed formats are a single plane of
/// `width * bytes_per_pixel`. Copying only plane 0 would silently drop the
/// chroma plane and emit a grayscale, half-length NV12 buffer.
///
/// The match is **exhaustive on purpose**: a future *planar* format (e.g. I420,
/// 3 separate planes) must not silently fall into the packed branch and drop its
/// chroma — adding a `PixelFormat` variant forces a conscious choice here.
pub fn pack_frame_into(
    dst: &mut Vec<u8>,
    frame: &ffmpeg_next::util::frame::Video,
    pf: PixelFormat,
) {
    dst.clear();
    let w = frame.width() as usize;
    match pf {
        PixelFormat::Nv12 => {
            copy_plane_tight(dst, frame, 0, w);
            copy_plane_tight(dst, frame, 1, w);
        }
        _ => copy_plane_tight(dst, frame, 0, w * pf.bytes_per_pixel()),
    }
}

fn copy_plane_tight(
    dst: &mut Vec<u8>,
    frame: &ffmpeg_next::util::frame::Video,
    plane: usize,
    tight_row: usize,
) {
    let stride = frame.stride(plane);
    let rows = frame.plane_height(plane) as usize;
    let data = frame.data(plane);
    if stride == tight_row {
        dst.extend_from_slice(&data[..tight_row * rows]);
    } else {
        for r in 0..rows {
            let off = r * stride;
            dst.extend_from_slice(&data[off..off + tight_row]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_frame_rgb_strips_stride_padding() {
        ffmpeg_next::init().ok();
        let (w, h) = (10u32, 4u32);
        let mut frame =
            ffmpeg_next::util::frame::Video::new(ffmpeg_next::format::Pixel::RGB24, w, h);
        let tight = (w * 3) as usize;
        let stride = frame.stride(0);
        for r in 0..h as usize {
            let row = &mut frame.data_mut(0)[r * stride..r * stride + tight];
            for b in row.iter_mut() {
                *b = r as u8;
            }
        }

        let mut dst = Vec::new();
        pack_frame_into(&mut dst, &frame, PixelFormat::Rgb8);

        assert_eq!(dst.len(), tight * h as usize, "no stride padding in output");
        for r in 0..h as usize {
            for c in 0..tight {
                assert_eq!(dst[r * tight + c], r as u8, "row {r} col {c} corrupted");
            }
        }
    }

    #[test]
    fn pack_frame_nv12_includes_chroma_plane() {
        ffmpeg_next::init().ok();
        let (w, h) = (16u32, 8u32);
        let mut frame =
            ffmpeg_next::util::frame::Video::new(ffmpeg_next::format::Pixel::NV12, w, h);

        let y_stride = frame.stride(0);
        for r in 0..h as usize {
            for c in 0..w as usize {
                frame.data_mut(0)[r * y_stride + c] = 1;
            }
        }
        let uv_stride = frame.stride(1);
        let uv_rows = (h / 2) as usize;
        for r in 0..uv_rows {
            for c in 0..w as usize {
                frame.data_mut(1)[r * uv_stride + c] = 2;
            }
        }

        let mut dst = Vec::new();
        pack_frame_into(&mut dst, &frame, PixelFormat::Nv12);

        let expect = (w * h + w * (h / 2)) as usize;
        assert_eq!(dst.len(), expect, "NV12 = Y (w*h) + interleaved UV (w*h/2)");
        let y_len = (w * h) as usize;
        assert!(dst[..y_len].iter().all(|&b| b == 1), "Y plane corrupted");
        assert!(
            dst[y_len..].iter().all(|&b| b == 2),
            "UV chroma plane missing"
        );
    }

    #[test]
    fn parse_pixel_format_aliases() {
        assert_eq!(parse_pixel_format("rgb24").unwrap().0, PixelFormat::Rgb8);
        assert_eq!(parse_pixel_format("BGR24").unwrap().0, PixelFormat::Bgr8);
        assert_eq!(parse_pixel_format("nv12").unwrap().0, PixelFormat::Nv12);
    }
}
