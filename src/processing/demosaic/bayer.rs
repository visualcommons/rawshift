//! Bayer-specific demosaicing algorithms.
//!
//! This module contains demosaicing algorithms designed for standard 2x2 Bayer
//! color filter arrays found in most cameras (Sony, Canon, Nikon, DNG, etc.).

use super::{Demosaic, DemosaicError};
use crate::core::image::{CfaPattern, RawImage};
use rayon::prelude::*;

// =============================================================================
// AMaZE — Aliasing Minimization and Zipper Elimination
// =============================================================================

/// AMaZE (Aliasing Minimization and Zipper Elimination) demosaicing algorithm.
///
/// Industry standard for high-detail, low-noise images. Uses directional
/// interpolation with adaptive selection based on local gradient homogeneity,
/// followed by color-difference reconstruction for non-green channels.
///
/// Key features:
/// - Gradient-based direction selection (horizontal vs vertical)
/// - Laplacian second-derivative correction for green interpolation
/// - Color-difference model for R/B reconstruction
/// - Homogeneity-driven blending at ambiguous edges
///
/// Reference: [RawTherapee AMaZE implementation](https://github.com/RawTherapee/RawTherapee)
pub struct Amaze;

impl Demosaic for Amaze {
    fn demosaic_into(&self, raw: &RawImage, output: &mut [u16]) -> Result<(), DemosaicError> {
        let width = raw.active_area.size.width as usize;
        let height = raw.active_area.size.height as usize;
        let x_off = raw.active_area.origin.x as usize;
        let y_off = raw.active_area.origin.y as usize;
        let raw_w = raw.size.width as usize;

        let expected_size = width * height * 3;
        if output.len() != expected_size {
            return Err(DemosaicError::BufferSizeMismatch {
                expected: expected_size,
                actual: output.len(),
            });
        }

        if width < 6 || height < 6 {
            return Err(DemosaicError::InvalidDimensions);
        }

        // Determine color at each CFA position: 0=Red, 1=Green(R-row), 2=Blue, 3=Green(B-row)
        let fc = |x: usize, y: usize| -> u8 {
            let ax = x + x_off;
            let ay = y + y_off;
            match raw.cfa_pattern {
                CfaPattern::Rggb => match (ax % 2, ay % 2) {
                    (0, 0) => 0, // R
                    (1, 0) => 1, // G on R-row
                    (0, 1) => 3, // G on B-row
                    _ => 2,      // B
                },
                CfaPattern::Grbg => match (ax % 2, ay % 2) {
                    (0, 0) => 1,
                    (1, 0) => 0,
                    (0, 1) => 2,
                    _ => 3,
                },
                CfaPattern::Gbrg => match (ax % 2, ay % 2) {
                    (0, 0) => 3,
                    (1, 0) => 2,
                    (0, 1) => 0,
                    _ => 1,
                },
                CfaPattern::Bggr => match (ax % 2, ay % 2) {
                    (0, 0) => 2,
                    (1, 0) => 3,
                    (0, 1) => 1,
                    _ => 0,
                },
            }
        };

        // Safe accessor into raw data with mirror-padding at borders
        let get = |x: isize, y: isize| -> f32 {
            let cx = x.clamp(0, (width as isize) - 1) as usize;
            let cy = y.clamp(0, (height as isize) - 1) as usize;
            raw.data[(cy + y_off) * raw_w + (cx + x_off)] as f32
        };

        // ── Step 1: Green channel interpolation ──────────────────────

        // Allocate green plane
        let mut green = vec![0.0f32; width * height];

        // For green pixels, just copy. For R/B pixels, interpolate green.
        for y in 0..height {
            for x in 0..width {
                let color = fc(x, y);
                let ix = x as isize;
                let iy = y as isize;

                if color == 1 || color == 3 {
                    // Green pixel — copy directly
                    green[y * width + x] = get(ix, iy);
                } else {
                    // Red or Blue pixel — interpolate green using directional gradients

                    // Horizontal gradient (Laplacian-weighted)
                    let dh = (get(ix - 1, iy) - get(ix + 1, iy)).abs()
                        + (2.0 * get(ix, iy) - get(ix - 2, iy) - get(ix + 2, iy)).abs();

                    // Vertical gradient
                    let dv = (get(ix, iy - 1) - get(ix, iy + 1)).abs()
                        + (2.0 * get(ix, iy) - get(ix, iy - 2) - get(ix, iy + 2)).abs();

                    // Horizontal green estimate with 2nd-derivative correction
                    let gh = (get(ix - 1, iy) + get(ix + 1, iy)) * 0.5
                        + (2.0 * get(ix, iy) - get(ix - 2, iy) - get(ix + 2, iy)) * 0.25;

                    // Vertical green estimate with 2nd-derivative correction
                    let gv = (get(ix, iy - 1) + get(ix, iy + 1)) * 0.5
                        + (2.0 * get(ix, iy) - get(ix, iy - 2) - get(ix, iy + 2)) * 0.25;

                    // Adaptive direction selection
                    let eps = 1e-5;
                    if dh < dv * 0.5 {
                        // Strong horizontal preference
                        green[y * width + x] = gh;
                    } else if dv < dh * 0.5 {
                        // Strong vertical preference
                        green[y * width + x] = gv;
                    } else {
                        // Blend based on gradient ratio
                        let wh = 1.0 / (dh + eps);
                        let wv = 1.0 / (dv + eps);
                        green[y * width + x] = (wh * gh + wv * gv) / (wh + wv);
                    }

                    // Clamp to valid range
                    green[y * width + x] = green[y * width + x].max(0.0);
                }
            }
        }

        // ── Step 2: Homogeneity-based green refinement ───────────────

        // Compute horizontal and vertical green estimates for homogeneity test
        let mut gh_plane = vec![0.0f32; width * height];
        let mut gv_plane = vec![0.0f32; width * height];

        for y in 0..height {
            for x in 0..width {
                let color = fc(x, y);
                let ix = x as isize;
                let iy = y as isize;

                if color == 1 || color == 3 {
                    gh_plane[y * width + x] = get(ix, iy);
                    gv_plane[y * width + x] = get(ix, iy);
                } else {
                    gh_plane[y * width + x] = (get(ix - 1, iy) + get(ix + 1, iy)) * 0.5
                        + (2.0 * get(ix, iy) - get(ix - 2, iy) - get(ix + 2, iy)) * 0.25;
                    gv_plane[y * width + x] = (get(ix, iy - 1) + get(ix, iy + 1)) * 0.5
                        + (2.0 * get(ix, iy) - get(ix, iy - 2) - get(ix, iy + 2)) * 0.25;
                }
            }
        }

        // Compute homogeneity in a 3x3 window around each pixel
        let mut h_homo = vec![0i32; width * height];
        let mut v_homo = vec![0i32; width * height];

        let border = 3usize;
        for y in border..height.saturating_sub(border) {
            for x in border..width.saturating_sub(border) {
                let color = fc(x, y);
                if color == 1 || color == 3 {
                    continue;
                }

                let mut hh = 0i32;
                let mut vh = 0i32;

                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        let nx = (x as i32 + dx) as usize;
                        let ny = (y as i32 + dy) as usize;
                        let idx = ny * width + nx;

                        // Color-difference homogeneity for horizontal
                        let cdh = (gh_plane[idx] - get(nx as isize, ny as isize)).abs();
                        // Color-difference homogeneity for vertical
                        let cdv = (gv_plane[idx] - get(nx as isize, ny as isize)).abs();

                        // Luminance homogeneity
                        let lh = (gh_plane[idx] - gh_plane[y * width + x]).abs();
                        let lv = (gv_plane[idx] - gv_plane[y * width + x]).abs();

                        let eps_h = cdh + lh;
                        let eps_v = cdv + lv;

                        if eps_h < eps_v {
                            hh += 1;
                        } else if eps_v < eps_h {
                            vh += 1;
                        }
                    }
                }

                h_homo[y * width + x] = hh;
                v_homo[y * width + x] = vh;
            }
        }

        // Refine green using homogeneity
        for y in border..height.saturating_sub(border) {
            for x in border..width.saturating_sub(border) {
                let color = fc(x, y);
                if color == 1 || color == 3 {
                    continue;
                }

                let idx = y * width + x;
                let hh = h_homo[idx];
                let vh = v_homo[idx];

                if hh > vh + 1 {
                    green[idx] = gh_plane[idx];
                } else if vh > hh + 1 {
                    green[idx] = gv_plane[idx];
                } else {
                    // Blend
                    let wh = (hh + 1) as f32;
                    let wv = (vh + 1) as f32;
                    green[idx] = (wh * gh_plane[idx] + wv * gv_plane[idx]) / (wh + wv);
                }

                green[idx] = green[idx].max(0.0);
            }
        }

        // Free intermediate buffers
        drop(gh_plane);
        drop(gv_plane);
        drop(h_homo);
        drop(v_homo);

        // ── Step 3: R/B channel reconstruction via color-difference ──

        // Build full R and B planes using color-difference interpolation.
        // Color differences (R-G) and (B-G) vary more smoothly than raw R/B,
        // so interpolating them produces fewer artifacts.

        let mut red = vec![0.0f32; width * height];
        let mut blue = vec![0.0f32; width * height];

        // First pass: fill in known values and compute color differences
        let mut cd_rg = vec![0.0f32; width * height]; // R - G
        let mut cd_bg = vec![0.0f32; width * height]; // B - G

        for y in 0..height {
            for x in 0..width {
                let idx = y * width + x;
                let color = fc(x, y);
                let val = get(x as isize, y as isize);

                match color {
                    0 => {
                        // Red pixel
                        red[idx] = val;
                        cd_rg[idx] = val - green[idx];
                    }
                    2 => {
                        // Blue pixel
                        blue[idx] = val;
                        cd_bg[idx] = val - green[idx];
                    }
                    _ => {}
                }
            }
        }

        // Second pass: interpolate color differences at missing positions
        // For green pixels on red rows: need both R and B via color-difference
        // For green pixels on blue rows: need both R and B via color-difference
        for y in 0..height {
            for x in 0..width {
                let idx = y * width + x;
                let color = fc(x, y);
                let ix = x as isize;
                let iy = y as isize;

                match color {
                    0 => {
                        // Red pixel — need blue. Blue neighbors are at diagonals.
                        let mut sum = 0.0f32;
                        let mut count = 0.0f32;
                        for &(dx, dy) in &[(-1i32, -1i32), (1, -1), (-1, 1), (1, 1)] {
                            let nx = ix + dx as isize;
                            let ny = iy + dy as isize;
                            if nx >= 0 && nx < width as isize && ny >= 0 && ny < height as isize {
                                let nidx = ny as usize * width + nx as usize;
                                if fc(nx as usize, ny as usize) == 2 {
                                    sum += cd_bg[nidx];
                                    count += 1.0;
                                }
                            }
                        }
                        if count > 0.0 {
                            blue[idx] = green[idx] + sum / count;
                        } else {
                            blue[idx] = green[idx];
                        }
                    }
                    2 => {
                        // Blue pixel — need red. Red neighbors are at diagonals.
                        let mut sum = 0.0f32;
                        let mut count = 0.0f32;
                        for &(dx, dy) in &[(-1i32, -1i32), (1, -1), (-1, 1), (1, 1)] {
                            let nx = ix + dx as isize;
                            let ny = iy + dy as isize;
                            if nx >= 0 && nx < width as isize && ny >= 0 && ny < height as isize {
                                let nidx = ny as usize * width + nx as usize;
                                if fc(nx as usize, ny as usize) == 0 {
                                    sum += cd_rg[nidx];
                                    count += 1.0;
                                }
                            }
                        }
                        if count > 0.0 {
                            red[idx] = green[idx] + sum / count;
                        } else {
                            red[idx] = green[idx];
                        }
                    }
                    1 => {
                        // Green on R-row — need R (horizontal neighbors) and B (vertical neighbors)
                        let mut sum_r = 0.0f32;
                        let mut cnt_r = 0.0f32;
                        for &dx in &[-1i32, 1] {
                            let nx = ix + dx as isize;
                            if nx >= 0 && nx < width as isize {
                                let nidx = y * width + nx as usize;
                                if fc(nx as usize, y) == 0 {
                                    sum_r += cd_rg[nidx];
                                    cnt_r += 1.0;
                                }
                            }
                        }
                        red[idx] = green[idx] + if cnt_r > 0.0 { sum_r / cnt_r } else { 0.0 };

                        let mut sum_b = 0.0f32;
                        let mut cnt_b = 0.0f32;
                        for &dy in &[-1i32, 1] {
                            let ny = iy + dy as isize;
                            if ny >= 0 && ny < height as isize {
                                let nidx = ny as usize * width + x;
                                if fc(x, ny as usize) == 2 {
                                    sum_b += cd_bg[nidx];
                                    cnt_b += 1.0;
                                }
                            }
                        }
                        blue[idx] = green[idx] + if cnt_b > 0.0 { sum_b / cnt_b } else { 0.0 };
                    }
                    3 => {
                        // Green on B-row — need B (horizontal neighbors) and R (vertical neighbors)
                        let mut sum_b = 0.0f32;
                        let mut cnt_b = 0.0f32;
                        for &dx in &[-1i32, 1] {
                            let nx = ix + dx as isize;
                            if nx >= 0 && nx < width as isize {
                                let nidx = y * width + nx as usize;
                                if fc(nx as usize, y) == 2 {
                                    sum_b += cd_bg[nidx];
                                    cnt_b += 1.0;
                                }
                            }
                        }
                        blue[idx] = green[idx] + if cnt_b > 0.0 { sum_b / cnt_b } else { 0.0 };

                        let mut sum_r = 0.0f32;
                        let mut cnt_r = 0.0f32;
                        for &dy in &[-1i32, 1] {
                            let ny = iy + dy as isize;
                            if ny >= 0 && ny < height as isize {
                                let nidx = ny as usize * width + x;
                                if fc(x, ny as usize) == 0 {
                                    sum_r += cd_rg[nidx];
                                    cnt_r += 1.0;
                                }
                            }
                        }
                        red[idx] = green[idx] + if cnt_r > 0.0 { sum_r / cnt_r } else { 0.0 };
                    }
                    _ => unreachable!(),
                }
            }
        }

        // ── Step 4: Zipper artifact reduction ────────────────────────

        // Detect and correct zipper artifacts by checking local color-difference
        // smoothness and replacing outliers with median-filtered values.
        let mut red_out = red.clone();
        let mut blue_out = blue.clone();

        for y in 2..height.saturating_sub(2) {
            for x in 2..width.saturating_sub(2) {
                let idx = y * width + x;
                let g = green[idx];

                // Check red-green difference smoothness
                let cd_r = red[idx] - g;
                let cd_r_h1 = red[idx.wrapping_sub(1)] - green[idx.wrapping_sub(1)];
                let cd_r_h2 = red[idx + 1] - green[idx + 1];
                let cd_r_v1 = red[(y - 1) * width + x] - green[(y - 1) * width + x];
                let cd_r_v2 = red[(y + 1) * width + x] - green[(y + 1) * width + x];

                // If center color-difference is an outlier, smooth it
                let avg_cd_r = (cd_r_h1 + cd_r_h2 + cd_r_v1 + cd_r_v2) * 0.25;
                let var_r = (cd_r_h1 - avg_cd_r).abs()
                    + (cd_r_h2 - avg_cd_r).abs()
                    + (cd_r_v1 - avg_cd_r).abs()
                    + (cd_r_v2 - avg_cd_r).abs();

                if (cd_r - avg_cd_r).abs() > var_r * 1.5 + 1.0 {
                    red_out[idx] = g + avg_cd_r;
                }

                // Same for blue-green
                let cd_b = blue[idx] - g;
                let cd_b_h1 = blue[idx.wrapping_sub(1)] - green[idx.wrapping_sub(1)];
                let cd_b_h2 = blue[idx + 1] - green[idx + 1];
                let cd_b_v1 = blue[(y - 1) * width + x] - green[(y - 1) * width + x];
                let cd_b_v2 = blue[(y + 1) * width + x] - green[(y + 1) * width + x];

                let avg_cd_b = (cd_b_h1 + cd_b_h2 + cd_b_v1 + cd_b_v2) * 0.25;
                let var_b = (cd_b_h1 - avg_cd_b).abs()
                    + (cd_b_h2 - avg_cd_b).abs()
                    + (cd_b_v1 - avg_cd_b).abs()
                    + (cd_b_v2 - avg_cd_b).abs();

                if (cd_b - avg_cd_b).abs() > var_b * 1.5 + 1.0 {
                    blue_out[idx] = g + avg_cd_b;
                }
            }
        }

        // ── Step 5: Write output ─────────────────────────────────────

        // Convert from planar f32 to interleaved u16 output
        output
            .par_chunks_mut(width * 3)
            .enumerate()
            .for_each(|(y, row)| {
                for x in 0..width {
                    let idx = y * width + x;
                    let out_idx = x * 3;
                    row[out_idx] = red_out[idx].round().clamp(0.0, 65535.0) as u16;
                    row[out_idx + 1] = green[idx].round().clamp(0.0, 65535.0) as u16;
                    row[out_idx + 2] = blue_out[idx].round().clamp(0.0, 65535.0) as u16;
                }
            });

        Ok(())
    }
}

// =============================================================================
// LMMSE — Linear Minimum Mean Square Error
// =============================================================================

/// LMMSE (Linear Minimum Mean Square Error) demosaicing algorithm.
///
/// High-ISO specialist that treats noise as a statistical probability.
/// Particularly effective for images shot at high ISO where noise is prominent.
pub struct Lmmse;

impl Demosaic for Lmmse {
    fn demosaic_into(&self, raw: &RawImage, output: &mut [u16]) -> Result<(), DemosaicError> {
        let width = raw.active_area.size.width as usize;
        let height = raw.active_area.size.height as usize;
        let x_off = raw.active_area.origin.x as usize;
        let y_off = raw.active_area.origin.y as usize;
        let raw_w = raw.size.width as usize;

        let expected_size = width * height * 3;
        if output.len() != expected_size {
            return Err(DemosaicError::BufferSizeMismatch {
                expected: expected_size,
                actual: output.len(),
            });
        }

        if width < 6 || height < 6 {
            return Err(DemosaicError::InvalidDimensions);
        }

        let white = raw.white_level as f32;

        // CFA color at each active-area position
        let fc = |x: usize, y: usize| -> u8 {
            let ax = x + x_off;
            let ay = y + y_off;
            match raw.cfa_pattern {
                CfaPattern::Rggb => match (ax % 2, ay % 2) {
                    (0, 0) => 0,
                    (1, 0) => 1,
                    (0, 1) => 3,
                    _ => 2,
                },
                CfaPattern::Grbg => match (ax % 2, ay % 2) {
                    (0, 0) => 1,
                    (1, 0) => 0,
                    (0, 1) => 2,
                    _ => 3,
                },
                CfaPattern::Gbrg => match (ax % 2, ay % 2) {
                    (0, 0) => 3,
                    (1, 0) => 2,
                    (0, 1) => 0,
                    _ => 1,
                },
                CfaPattern::Bggr => match (ax % 2, ay % 2) {
                    (0, 0) => 2,
                    (1, 0) => 3,
                    (0, 1) => 1,
                    _ => 0,
                },
            }
        };

        // Mirror-padded accessor into the raw data (active area coordinates)
        let get = |x: isize, y: isize| -> f32 {
            let cx = x.clamp(0, (width as isize) - 1) as usize;
            let cy = y.clamp(0, (height as isize) - 1) as usize;
            raw.data[(cy + y_off) * raw_w + (cx + x_off)] as f32
        };

        // ── Step 1: Compute horizontal and vertical green estimates ──

        let mut gh = vec![0.0f32; width * height];
        let mut gv = vec![0.0f32; width * height];

        for y in 0..height {
            for x in 0..width {
                let color = fc(x, y);
                let ix = x as isize;
                let iy = y as isize;

                if color == 1 || color == 3 {
                    // Green pixel — copy to both directional planes
                    let val = get(ix, iy);
                    gh[y * width + x] = val;
                    gv[y * width + x] = val;
                } else {
                    // Non-green pixel — LMMSE horizontal estimate
                    // gh = 0.5*(G[x-1]+G[x+1]) + 0.25*(2*raw[x]-raw[x-2]-raw[x+2])
                    let est_h = 0.5 * (get(ix - 1, iy) + get(ix + 1, iy))
                        + 0.25 * (2.0 * get(ix, iy) - get(ix - 2, iy) - get(ix + 2, iy));
                    gh[y * width + x] = est_h.clamp(0.0, white);

                    // LMMSE vertical estimate
                    let est_v = 0.5 * (get(ix, iy - 1) + get(ix, iy + 1))
                        + 0.25 * (2.0 * get(ix, iy) - get(ix, iy - 2) - get(ix, iy + 2));
                    gv[y * width + x] = est_v.clamp(0.0, white);
                }
            }
        }

        // ── Step 2: Variance-based adaptive blending of green estimates ──

        let mut green = vec![0.0f32; width * height];
        let eps = 1e-5f32;
        let half_win = 2usize; // 5-pixel window radius

        for y in 0..height {
            for x in 0..width {
                let color = fc(x, y);
                if color == 1 || color == 3 {
                    green[y * width + x] = gh[y * width + x]; // already the raw value
                    continue;
                }

                // Compute local variance of (raw - green_est) along each direction
                let mut sum_h = 0.0f32;
                let mut sum_sq_h = 0.0f32;
                let mut sum_v = 0.0f32;
                let mut sum_sq_v = 0.0f32;
                let mut n = 0.0f32;

                for dy in -(half_win as isize)..=(half_win as isize) {
                    for dx in -(half_win as isize)..=(half_win as isize) {
                        let nx = (x as isize + dx).clamp(0, (width as isize) - 1) as usize;
                        let ny = (y as isize + dy).clamp(0, (height as isize) - 1) as usize;
                        let nidx = ny * width + nx;
                        let raw_val = get(nx as isize, ny as isize);
                        let dh = raw_val - gh[nidx];
                        let dv = raw_val - gv[nidx];
                        sum_h += dh;
                        sum_sq_h += dh * dh;
                        sum_v += dv;
                        sum_sq_v += dv * dv;
                        n += 1.0;
                    }
                }

                let var_h = (sum_sq_h - sum_h * sum_h / n) / n;
                let var_v = (sum_sq_v - sum_v * sum_v / n) / n;

                // Weight inversely proportional to variance
                let wh = 1.0 / (var_h + eps);
                let wv = 1.0 / (var_v + eps);

                let idx = y * width + x;
                green[idx] = ((wh * gh[idx] + wv * gv[idx]) / (wh + wv)).clamp(0.0, white);
            }
        }

        drop(gh);
        drop(gv);

        // ── Step 3: R/B interpolation via color-difference bilinear ──

        // Compute color differences at known positions, then interpolate.
        let mut cd_rg = vec![0.0f32; width * height]; // R - G at R pixels
        let mut cd_bg = vec![0.0f32; width * height]; // B - G at B pixels

        for y in 0..height {
            for x in 0..width {
                let idx = y * width + x;
                match fc(x, y) {
                    0 => cd_rg[idx] = get(x as isize, y as isize) - green[idx],
                    2 => cd_bg[idx] = get(x as isize, y as isize) - green[idx],
                    _ => {}
                }
            }
        }

        let mut red = vec![0.0f32; width * height];
        let mut blue = vec![0.0f32; width * height];

        for y in 0..height {
            for x in 0..width {
                let idx = y * width + x;
                let color = fc(x, y);
                let ix = x as isize;
                let iy = y as isize;

                match color {
                    0 => {
                        // Red pixel — R is known, need B from diagonal bilinear
                        red[idx] = get(ix, iy);
                        let mut sum = 0.0f32;
                        let mut cnt = 0.0f32;
                        for &(dx, dy) in &[(-1i32, -1i32), (1, -1), (-1, 1), (1, 1)] {
                            let nx = ix + dx as isize;
                            let ny = iy + dy as isize;
                            if nx >= 0 && nx < width as isize && ny >= 0 && ny < height as isize {
                                let nidx = ny as usize * width + nx as usize;
                                if fc(nx as usize, ny as usize) == 2 {
                                    sum += cd_bg[nidx];
                                    cnt += 1.0;
                                }
                            }
                        }
                        blue[idx] = (green[idx] + if cnt > 0.0 { sum / cnt } else { 0.0 })
                            .clamp(0.0, white);
                    }
                    2 => {
                        // Blue pixel — B is known, need R from diagonal bilinear
                        blue[idx] = get(ix, iy);
                        let mut sum = 0.0f32;
                        let mut cnt = 0.0f32;
                        for &(dx, dy) in &[(-1i32, -1i32), (1, -1), (-1, 1), (1, 1)] {
                            let nx = ix + dx as isize;
                            let ny = iy + dy as isize;
                            if nx >= 0 && nx < width as isize && ny >= 0 && ny < height as isize {
                                let nidx = ny as usize * width + nx as usize;
                                if fc(nx as usize, ny as usize) == 0 {
                                    sum += cd_rg[nidx];
                                    cnt += 1.0;
                                }
                            }
                        }
                        red[idx] = (green[idx] + if cnt > 0.0 { sum / cnt } else { 0.0 })
                            .clamp(0.0, white);
                    }
                    1 => {
                        // Green on R-row: R from horizontal neighbors, B from vertical
                        let mut sr = 0.0f32;
                        let mut cr = 0.0f32;
                        for &dx in &[-1i32, 1] {
                            let nx = ix + dx as isize;
                            if nx >= 0 && nx < width as isize {
                                let nidx = y * width + nx as usize;
                                if fc(nx as usize, y) == 0 {
                                    sr += cd_rg[nidx];
                                    cr += 1.0;
                                }
                            }
                        }
                        red[idx] =
                            (green[idx] + if cr > 0.0 { sr / cr } else { 0.0 }).clamp(0.0, white);

                        let mut sb = 0.0f32;
                        let mut cb = 0.0f32;
                        for &dy in &[-1i32, 1] {
                            let ny = iy + dy as isize;
                            if ny >= 0 && ny < height as isize {
                                let nidx = ny as usize * width + x;
                                if fc(x, ny as usize) == 2 {
                                    sb += cd_bg[nidx];
                                    cb += 1.0;
                                }
                            }
                        }
                        blue[idx] =
                            (green[idx] + if cb > 0.0 { sb / cb } else { 0.0 }).clamp(0.0, white);
                    }
                    3 => {
                        // Green on B-row: B from horizontal neighbors, R from vertical
                        let mut sb = 0.0f32;
                        let mut cb = 0.0f32;
                        for &dx in &[-1i32, 1] {
                            let nx = ix + dx as isize;
                            if nx >= 0 && nx < width as isize {
                                let nidx = y * width + nx as usize;
                                if fc(nx as usize, y) == 2 {
                                    sb += cd_bg[nidx];
                                    cb += 1.0;
                                }
                            }
                        }
                        blue[idx] =
                            (green[idx] + if cb > 0.0 { sb / cb } else { 0.0 }).clamp(0.0, white);

                        let mut sr = 0.0f32;
                        let mut cr = 0.0f32;
                        for &dy in &[-1i32, 1] {
                            let ny = iy + dy as isize;
                            if ny >= 0 && ny < height as isize {
                                let nidx = ny as usize * width + x;
                                if fc(x, ny as usize) == 0 {
                                    sr += cd_rg[nidx];
                                    cr += 1.0;
                                }
                            }
                        }
                        red[idx] =
                            (green[idx] + if cr > 0.0 { sr / cr } else { 0.0 }).clamp(0.0, white);
                    }
                    _ => unreachable!(),
                }
            }
        }

        // ── Step 4: Write interleaved RGB output ──────────────────────

        output
            .par_chunks_mut(width * 3)
            .enumerate()
            .for_each(|(y, row)| {
                for x in 0..width {
                    let idx = y * width + x;
                    let out = x * 3;
                    row[out] = red[idx].round().clamp(0.0, white) as u16;
                    row[out + 1] = green[idx].round().clamp(0.0, white) as u16;
                    row[out + 2] = blue[idx].round().clamp(0.0, white) as u16;
                }
            });

        Ok(())
    }
}

// =============================================================================
// RCD — Ratio Corrected Demosaicing
// =============================================================================

/// RCD (Ratio Corrected Demosaicing) algorithm.
///
/// Fast, high-quality alternative to AMaZE that's particularly good for
/// organic shapes and natural textures.
pub struct Rcd;

impl Demosaic for Rcd {
    fn demosaic_into(&self, raw: &RawImage, output: &mut [u16]) -> Result<(), DemosaicError> {
        let width = raw.active_area.size.width as usize;
        let height = raw.active_area.size.height as usize;
        let x_off = raw.active_area.origin.x as usize;
        let y_off = raw.active_area.origin.y as usize;
        let raw_w = raw.size.width as usize;

        let expected_size = width * height * 3;
        if output.len() != expected_size {
            return Err(DemosaicError::BufferSizeMismatch {
                expected: expected_size,
                actual: output.len(),
            });
        }

        if width < 6 || height < 6 {
            return Err(DemosaicError::InvalidDimensions);
        }

        let white = raw.white_level as f32;

        // CFA color at each active-area position
        let fc = |x: usize, y: usize| -> u8 {
            let ax = x + x_off;
            let ay = y + y_off;
            match raw.cfa_pattern {
                CfaPattern::Rggb => match (ax % 2, ay % 2) {
                    (0, 0) => 0,
                    (1, 0) => 1,
                    (0, 1) => 3,
                    _ => 2,
                },
                CfaPattern::Grbg => match (ax % 2, ay % 2) {
                    (0, 0) => 1,
                    (1, 0) => 0,
                    (0, 1) => 2,
                    _ => 3,
                },
                CfaPattern::Gbrg => match (ax % 2, ay % 2) {
                    (0, 0) => 3,
                    (1, 0) => 2,
                    (0, 1) => 0,
                    _ => 1,
                },
                CfaPattern::Bggr => match (ax % 2, ay % 2) {
                    (0, 0) => 2,
                    (1, 0) => 3,
                    (0, 1) => 1,
                    _ => 0,
                },
            }
        };

        // Mirror-padded accessor
        let get = |x: isize, y: isize| -> f32 {
            let cx = x.clamp(0, (width as isize) - 1) as usize;
            let cy = y.clamp(0, (height as isize) - 1) as usize;
            raw.data[(cy + y_off) * raw_w + (cx + x_off)] as f32
        };

        // ── Step 1: Green channel interpolation (adaptive directional) ──

        let mut green = vec![0.0f32; width * height];

        for y in 0..height {
            for x in 0..width {
                let color = fc(x, y);
                let ix = x as isize;
                let iy = y as isize;

                if color == 1 || color == 3 {
                    green[y * width + x] = get(ix, iy);
                } else {
                    // RCD uses same adaptive directional green interpolation as AMaZE
                    let dh = (get(ix - 1, iy) - get(ix + 1, iy)).abs()
                        + (2.0 * get(ix, iy) - get(ix - 2, iy) - get(ix + 2, iy)).abs();
                    let dv = (get(ix, iy - 1) - get(ix, iy + 1)).abs()
                        + (2.0 * get(ix, iy) - get(ix, iy - 2) - get(ix, iy + 2)).abs();

                    let gh = (get(ix - 1, iy) + get(ix + 1, iy)) * 0.5
                        + (2.0 * get(ix, iy) - get(ix - 2, iy) - get(ix + 2, iy)) * 0.25;
                    let gv = (get(ix, iy - 1) + get(ix, iy + 1)) * 0.5
                        + (2.0 * get(ix, iy) - get(ix, iy - 2) - get(ix, iy + 2)) * 0.25;

                    let eps = 1e-5f32;
                    let g = if dh < dv * 0.5 {
                        gh
                    } else if dv < dh * 0.5 {
                        gv
                    } else {
                        let wh = 1.0 / (dh + eps);
                        let wv = 1.0 / (dv + eps);
                        (wh * gh + wv * gv) / (wh + wv)
                    };
                    green[y * width + x] = g.max(0.0);
                }
            }
        }

        // ── Step 2: R/B reconstruction using color ratios ─────────────

        // The RCD insight: R/G and B/G ratios are smoother than R-G differences.
        // We interpolate ratios and then multiply back by the interpolated green.

        // First build ratio planes at known positions.
        // ratio_rg[idx] = R[idx] / G[idx] at R pixels (else 1.0 placeholder)
        // ratio_bg[idx] = B[idx] / G[idx] at B pixels (else 1.0 placeholder)
        let mut ratio_rg = vec![1.0f32; width * height];
        let mut ratio_bg = vec![1.0f32; width * height];

        for y in 0..height {
            for x in 0..width {
                let idx = y * width + x;
                let g = green[idx].max(1.0); // avoid division by zero
                match fc(x, y) {
                    0 => ratio_rg[idx] = get(x as isize, y as isize) / g,
                    2 => ratio_bg[idx] = get(x as isize, y as isize) / g,
                    _ => {}
                }
            }
        }

        let mut red = vec![0.0f32; width * height];
        let mut blue = vec![0.0f32; width * height];

        for y in 0..height {
            for x in 0..width {
                let idx = y * width + x;
                let color = fc(x, y);
                let ix = x as isize;
                let iy = y as isize;

                match color {
                    0 => {
                        // R pixel — R is known; B interpolated from diagonal ratio bilinear
                        red[idx] = get(ix, iy);
                        let mut sum_ratio = 0.0f32;
                        let mut cnt = 0.0f32;
                        for &(dx, dy) in &[(-1i32, -1i32), (1, -1), (-1, 1), (1, 1)] {
                            let nx = ix + dx as isize;
                            let ny = iy + dy as isize;
                            if nx >= 0
                                && nx < width as isize
                                && ny >= 0
                                && ny < height as isize
                                && fc(nx as usize, ny as usize) == 2
                            {
                                sum_ratio += ratio_bg[ny as usize * width + nx as usize];
                                cnt += 1.0;
                            }
                        }
                        let r = if cnt > 0.0 { sum_ratio / cnt } else { 1.0 };
                        blue[idx] = (green[idx] * r).clamp(0.0, white);
                    }
                    2 => {
                        // B pixel — B is known; R interpolated from diagonal ratio bilinear
                        blue[idx] = get(ix, iy);
                        let mut sum_ratio = 0.0f32;
                        let mut cnt = 0.0f32;
                        for &(dx, dy) in &[(-1i32, -1i32), (1, -1), (-1, 1), (1, 1)] {
                            let nx = ix + dx as isize;
                            let ny = iy + dy as isize;
                            if nx >= 0
                                && nx < width as isize
                                && ny >= 0
                                && ny < height as isize
                                && fc(nx as usize, ny as usize) == 0
                            {
                                sum_ratio += ratio_rg[ny as usize * width + nx as usize];
                                cnt += 1.0;
                            }
                        }
                        let r = if cnt > 0.0 { sum_ratio / cnt } else { 1.0 };
                        red[idx] = (green[idx] * r).clamp(0.0, white);
                    }
                    1 => {
                        // Green on R-row:
                        // R from horizontal R/G ratio pairs
                        // B from vertical B/G ratio pairs
                        let mut sr = 0.0f32;
                        let mut cr = 0.0f32;
                        for &dx in &[-1i32, 1] {
                            let nx = ix + dx as isize;
                            if nx >= 0 && nx < width as isize && fc(nx as usize, y) == 0 {
                                sr += ratio_rg[y * width + nx as usize];
                                cr += 1.0;
                            }
                        }
                        let rr = if cr > 0.0 { sr / cr } else { 1.0 };
                        red[idx] = (green[idx] * rr).clamp(0.0, white);

                        let mut sb = 0.0f32;
                        let mut cb = 0.0f32;
                        for &dy in &[-1i32, 1] {
                            let ny = iy + dy as isize;
                            if ny >= 0 && ny < height as isize && fc(x, ny as usize) == 2 {
                                sb += ratio_bg[ny as usize * width + x];
                                cb += 1.0;
                            }
                        }
                        let rb = if cb > 0.0 { sb / cb } else { 1.0 };
                        blue[idx] = (green[idx] * rb).clamp(0.0, white);
                    }
                    3 => {
                        // Green on B-row:
                        // B from horizontal B/G ratio pairs
                        // R from vertical R/G ratio pairs
                        let mut sb = 0.0f32;
                        let mut cb = 0.0f32;
                        for &dx in &[-1i32, 1] {
                            let nx = ix + dx as isize;
                            if nx >= 0 && nx < width as isize && fc(nx as usize, y) == 2 {
                                sb += ratio_bg[y * width + nx as usize];
                                cb += 1.0;
                            }
                        }
                        let rb = if cb > 0.0 { sb / cb } else { 1.0 };
                        blue[idx] = (green[idx] * rb).clamp(0.0, white);

                        let mut sr = 0.0f32;
                        let mut cr = 0.0f32;
                        for &dy in &[-1i32, 1] {
                            let ny = iy + dy as isize;
                            if ny >= 0 && ny < height as isize && fc(x, ny as usize) == 0 {
                                sr += ratio_rg[ny as usize * width + x];
                                cr += 1.0;
                            }
                        }
                        let rr = if cr > 0.0 { sr / cr } else { 1.0 };
                        red[idx] = (green[idx] * rr).clamp(0.0, white);
                    }
                    _ => unreachable!(),
                }
            }
        }

        // ── Step 3: Write interleaved RGB output ──────────────────────

        output
            .par_chunks_mut(width * 3)
            .enumerate()
            .for_each(|(y, row)| {
                for x in 0..width {
                    let idx = y * width + x;
                    let out = x * 3;
                    row[out] = red[idx].round().clamp(0.0, white) as u16;
                    row[out + 1] = green[idx].round().clamp(0.0, white) as u16;
                    row[out + 2] = blue[idx].round().clamp(0.0, white) as u16;
                }
            });

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::image::{Point, Rect, Size};

    fn create_test_raw(width: u32, height: u32, pattern: CfaPattern, value: u16) -> RawImage {
        let size = Size::new(width, height);
        let active_area = Rect::new(Point::ORIGIN, size);
        RawImage {
            size,
            active_area,
            bit_depth: 14,
            cfa_pattern: pattern,
            xtrans_pattern: None,
            black_levels: [0; 4],
            white_level: 16383,
            data: vec![value; (width * height) as usize],
            baseline_exposure: None,
            default_crop: None,
        }
    }

    fn create_gradient_raw(width: u32, height: u32, pattern: CfaPattern) -> RawImage {
        let size = Size::new(width, height);
        let active_area = Rect::new(Point::ORIGIN, size);
        let mut data = vec![0u16; (width * height) as usize];
        for y in 0..height {
            for x in 0..width {
                // Smooth gradient — easy for demosaic
                let val =
                    ((x as f32 / width as f32 + y as f32 / height as f32) * 0.5 * 8000.0) as u16;
                data[(y * width + x) as usize] = val;
            }
        }
        RawImage {
            size,
            active_area,
            bit_depth: 14,
            cfa_pattern: pattern,
            xtrans_pattern: None,
            black_levels: [0; 4],
            white_level: 16383,
            data,
            baseline_exposure: None,
            default_crop: None,
        }
    }

    #[test]
    fn test_amaze_correct_output_size() {
        let raw = create_test_raw(20, 20, CfaPattern::Rggb, 5000);
        let mut output = vec![0u16; 20 * 20 * 3];
        assert!(Amaze.demosaic_into(&raw, &mut output).is_ok());
    }

    #[test]
    fn test_amaze_wrong_buffer_size() {
        let raw = create_test_raw(20, 20, CfaPattern::Rggb, 5000);
        let mut output = vec![0u16; 50];
        assert!(matches!(
            Amaze.demosaic_into(&raw, &mut output),
            Err(DemosaicError::BufferSizeMismatch { .. })
        ));
    }

    #[test]
    fn test_amaze_too_small_image() {
        let raw = create_test_raw(4, 4, CfaPattern::Rggb, 5000);
        let mut output = vec![0u16; 4 * 4 * 3];
        assert!(matches!(
            Amaze.demosaic_into(&raw, &mut output),
            Err(DemosaicError::InvalidDimensions)
        ));
    }

    #[test]
    fn test_amaze_uniform_produces_uniform() {
        let raw = create_test_raw(20, 20, CfaPattern::Rggb, 5000);
        let rgb = Amaze.demosaic(&raw);

        // Interior pixels should be close to 5000
        for y in 4..16 {
            for x in 4..16 {
                let idx = (y * 20 + x) * 3;
                for c in 0..3 {
                    let val = rgb.data[idx + c];
                    assert!(
                        (val as i32 - 5000).abs() < 500,
                        "pixel ({},{}) ch {} = {}, expected ~5000",
                        x,
                        y,
                        c,
                        val
                    );
                }
            }
        }
    }

    #[test]
    fn test_amaze_all_cfa_patterns() {
        for pattern in [
            CfaPattern::Rggb,
            CfaPattern::Grbg,
            CfaPattern::Gbrg,
            CfaPattern::Bggr,
        ] {
            let raw = create_test_raw(20, 20, pattern, 3000);
            let rgb = Amaze.demosaic(&raw);
            assert_eq!(rgb.width, 20);
            assert_eq!(rgb.height, 20);
            assert_eq!(rgb.data.len(), 20 * 20 * 3);

            // All values should be non-negative and bounded
            for val in &rgb.data {
                assert!(
                    *val <= 16383,
                    "pattern {:?}: value {} too high",
                    pattern,
                    val
                );
            }
        }
    }

    #[test]
    fn test_amaze_gradient_smooth() {
        let raw = create_gradient_raw(40, 40, CfaPattern::Rggb);
        let rgb = Amaze.demosaic(&raw);

        // Check that output is reasonably smooth (no huge jumps between neighbors)
        for y in 5..35 {
            for x in 5..35 {
                let idx = (y * 40 + x) * 3;
                let idx_right = (y * 40 + x + 1) * 3;
                let idx_down = ((y + 1) * 40 + x) * 3;

                for c in 0..3 {
                    let diff_h = (rgb.data[idx + c] as i32 - rgb.data[idx_right + c] as i32).abs();
                    let diff_v = (rgb.data[idx + c] as i32 - rgb.data[idx_down + c] as i32).abs();
                    assert!(
                        diff_h < 1000,
                        "horizontal jump at ({},{}) ch {}: {}",
                        x,
                        y,
                        c,
                        diff_h
                    );
                    assert!(
                        diff_v < 1000,
                        "vertical jump at ({},{}) ch {}: {}",
                        x,
                        y,
                        c,
                        diff_v
                    );
                }
            }
        }
    }

    #[test]
    fn test_amaze_preserves_known_green() {
        // Create image where all values are distinct
        let mut raw = create_test_raw(10, 10, CfaPattern::Rggb, 0);
        // Set known green pixel at (1,0) which is G in RGGB
        raw.data[1] = 7000;
        let rgb = Amaze.demosaic(&raw);
        // Green channel at (1,0) should be exactly 7000
        // pixel (1, 0): row=0, col=1, so index = 1 * 3 + 1 = 4
        let g = rgb.data[1 * 3 + 1];
        assert_eq!(
            g, 7000,
            "green pixel should be preserved exactly, got {}",
            g
        );
    }

    #[test]
    fn test_amaze_with_active_area() {
        let mut raw = create_test_raw(30, 30, CfaPattern::Rggb, 4000);
        raw.active_area = Rect::from_coords(5, 5, 20, 20);
        let rgb = Amaze.demosaic(&raw);
        assert_eq!(rgb.width, 20);
        assert_eq!(rgb.height, 20);
        assert_eq!(rgb.data.len(), 20 * 20 * 3);
    }

    // ── LMMSE tests ───────────────────────────────────────────────────────────

    #[test]
    fn test_lmmse_correct_output_size() {
        let raw = create_test_raw(20, 20, CfaPattern::Rggb, 5000);
        let mut output = vec![0u16; 20 * 20 * 3];
        assert!(Lmmse.demosaic_into(&raw, &mut output).is_ok());
    }

    #[test]
    fn test_lmmse_wrong_buffer_size() {
        let raw = create_test_raw(20, 20, CfaPattern::Rggb, 5000);
        let mut output = vec![0u16; 50];
        assert!(matches!(
            Lmmse.demosaic_into(&raw, &mut output),
            Err(DemosaicError::BufferSizeMismatch { .. })
        ));
    }

    #[test]
    fn test_lmmse_too_small_image() {
        let raw = create_test_raw(4, 4, CfaPattern::Rggb, 5000);
        let mut output = vec![0u16; 4 * 4 * 3];
        assert!(matches!(
            Lmmse.demosaic_into(&raw, &mut output),
            Err(DemosaicError::InvalidDimensions)
        ));
    }

    #[test]
    fn test_lmmse_uniform_produces_uniform() {
        let raw = create_test_raw(20, 20, CfaPattern::Rggb, 5000);
        let rgb = Lmmse.demosaic(&raw);

        for y in 4..16 {
            for x in 4..16 {
                let idx = (y * 20 + x) * 3;
                for c in 0..3 {
                    let val = rgb.data[idx + c];
                    assert!(
                        (val as i32 - 5000).abs() < 500,
                        "LMMSE pixel ({},{}) ch {} = {}, expected ~5000",
                        x,
                        y,
                        c,
                        val
                    );
                }
            }
        }
    }

    #[test]
    fn test_lmmse_all_cfa_patterns() {
        for pattern in [
            CfaPattern::Rggb,
            CfaPattern::Grbg,
            CfaPattern::Gbrg,
            CfaPattern::Bggr,
        ] {
            let raw = create_test_raw(20, 20, pattern, 3000);
            let rgb = Lmmse.demosaic(&raw);
            assert_eq!(rgb.width, 20);
            assert_eq!(rgb.height, 20);
            assert_eq!(rgb.data.len(), 20 * 20 * 3);

            for val in &rgb.data {
                assert!(
                    *val <= 16383,
                    "LMMSE pattern {:?}: value {} too high",
                    pattern,
                    val
                );
            }
        }
    }

    #[test]
    fn test_lmmse_gradient_smooth() {
        let raw = create_gradient_raw(40, 40, CfaPattern::Rggb);
        let rgb = Lmmse.demosaic(&raw);

        for y in 5..35 {
            for x in 5..35 {
                let idx = (y * 40 + x) * 3;
                let idx_right = (y * 40 + x + 1) * 3;
                let idx_down = ((y + 1) * 40 + x) * 3;

                for c in 0..3 {
                    let diff_h = (rgb.data[idx + c] as i32 - rgb.data[idx_right + c] as i32).abs();
                    let diff_v = (rgb.data[idx + c] as i32 - rgb.data[idx_down + c] as i32).abs();
                    assert!(
                        diff_h < 1000,
                        "LMMSE horizontal jump at ({},{}) ch {}: {}",
                        x,
                        y,
                        c,
                        diff_h
                    );
                    assert!(
                        diff_v < 1000,
                        "LMMSE vertical jump at ({},{}) ch {}: {}",
                        x,
                        y,
                        c,
                        diff_v
                    );
                }
            }
        }
    }

    #[test]
    fn test_lmmse_with_active_area() {
        let mut raw = create_test_raw(30, 30, CfaPattern::Rggb, 4000);
        raw.active_area = Rect::from_coords(5, 5, 20, 20);
        let rgb = Lmmse.demosaic(&raw);
        assert_eq!(rgb.width, 20);
        assert_eq!(rgb.height, 20);
        assert_eq!(rgb.data.len(), 20 * 20 * 3);
    }

    // ── RCD tests ─────────────────────────────────────────────────────────────

    #[test]
    fn test_rcd_correct_output_size() {
        let raw = create_test_raw(20, 20, CfaPattern::Rggb, 5000);
        let mut output = vec![0u16; 20 * 20 * 3];
        assert!(Rcd.demosaic_into(&raw, &mut output).is_ok());
    }

    #[test]
    fn test_rcd_wrong_buffer_size() {
        let raw = create_test_raw(20, 20, CfaPattern::Rggb, 5000);
        let mut output = vec![0u16; 50];
        assert!(matches!(
            Rcd.demosaic_into(&raw, &mut output),
            Err(DemosaicError::BufferSizeMismatch { .. })
        ));
    }

    #[test]
    fn test_rcd_too_small_image() {
        let raw = create_test_raw(4, 4, CfaPattern::Rggb, 5000);
        let mut output = vec![0u16; 4 * 4 * 3];
        assert!(matches!(
            Rcd.demosaic_into(&raw, &mut output),
            Err(DemosaicError::InvalidDimensions)
        ));
    }

    #[test]
    fn test_rcd_uniform_produces_uniform() {
        let raw = create_test_raw(20, 20, CfaPattern::Rggb, 5000);
        let rgb = Rcd.demosaic(&raw);

        for y in 4..16 {
            for x in 4..16 {
                let idx = (y * 20 + x) * 3;
                for c in 0..3 {
                    let val = rgb.data[idx + c];
                    assert!(
                        (val as i32 - 5000).abs() < 500,
                        "RCD pixel ({},{}) ch {} = {}, expected ~5000",
                        x,
                        y,
                        c,
                        val
                    );
                }
            }
        }
    }

    #[test]
    fn test_rcd_all_cfa_patterns() {
        for pattern in [
            CfaPattern::Rggb,
            CfaPattern::Grbg,
            CfaPattern::Gbrg,
            CfaPattern::Bggr,
        ] {
            let raw = create_test_raw(20, 20, pattern, 3000);
            let rgb = Rcd.demosaic(&raw);
            assert_eq!(rgb.width, 20);
            assert_eq!(rgb.height, 20);
            assert_eq!(rgb.data.len(), 20 * 20 * 3);

            for val in &rgb.data {
                assert!(
                    *val <= 16383,
                    "RCD pattern {:?}: value {} too high",
                    pattern,
                    val
                );
            }
        }
    }

    #[test]
    fn test_rcd_gradient_smooth() {
        let raw = create_gradient_raw(40, 40, CfaPattern::Rggb);
        let rgb = Rcd.demosaic(&raw);

        for y in 5..35 {
            for x in 5..35 {
                let idx = (y * 40 + x) * 3;
                let idx_right = (y * 40 + x + 1) * 3;
                let idx_down = ((y + 1) * 40 + x) * 3;

                for c in 0..3 {
                    let diff_h = (rgb.data[idx + c] as i32 - rgb.data[idx_right + c] as i32).abs();
                    let diff_v = (rgb.data[idx + c] as i32 - rgb.data[idx_down + c] as i32).abs();
                    assert!(
                        diff_h < 1000,
                        "RCD horizontal jump at ({},{}) ch {}: {}",
                        x,
                        y,
                        c,
                        diff_h
                    );
                    assert!(
                        diff_v < 1000,
                        "RCD vertical jump at ({},{}) ch {}: {}",
                        x,
                        y,
                        c,
                        diff_v
                    );
                }
            }
        }
    }

    #[test]
    fn test_rcd_with_active_area() {
        let mut raw = create_test_raw(30, 30, CfaPattern::Rggb, 4000);
        raw.active_area = Rect::from_coords(5, 5, 20, 20);
        let rgb = Rcd.demosaic(&raw);
        assert_eq!(rgb.width, 20);
        assert_eq!(rgb.height, 20);
        assert_eq!(rgb.data.len(), 20 * 20 * 3);
    }
}
