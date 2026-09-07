// SPDX-License-Identifier: MIT OR Apache-2.0
//! Drawing a single widget.
//!
//! Split from [`super::compose()`] so the tree walk and the per-widget
//! painting can be tested apart: a stacking bug and a bar-geometry bug look
//! identical on screen and are found in completely different places.

use crate::font::Font;
use crate::render::scale::scale_nearest;
use crate::widget::{Align, Kind, VAlign};
use crate::{Color, Rect, Surface};

use super::{Resources, blit, fill_rect, stroke_rect};

/// Draw `text` with its top-left at `at`, clipped to `clip`.
///
/// Glyphs are emitted one horizontal run at a time rather than one pixel at a
/// time: a run of ink in a row becomes a single `fill_span`, which on a
/// backend where a span is a bus transaction is the difference between five
/// writes per row and one.
///
/// Text that runs past `at` is not wrapped. A label is a fixed box in a scene
/// and silently reflowing it would move things the author positioned by hand;
/// the overflow is simply clipped.
/// How far in from the left edge the first glyph starts.
///
/// The ink of `n` glyphs is `n - 1` advances plus one cell: the gap after the
/// last glyph is spacing between it and the next, and counting it would push
/// centred text half a space to the left of where it belongs.
fn offset(text: &str, at: Rect, advance: i32, cell: i32, align: Align) -> i32 {
    if matches!(align, Align::Left) {
        return 0;
    }
    let n = text.chars().count() as i32;
    if n == 0 {
        return 0;
    }
    let ink = (n - 1) * advance + cell;
    let slack = at.size.w as i32 - ink;
    // Never negative: text too wide for its box starts at the left edge and
    // runs off the right, which is the one direction a reader can predict.
    match align {
        Align::Left => 0,
        Align::Center => (slack / 2).max(0),
        Align::Right => slack.max(0),
    }
}

#[allow(clippy::too_many_arguments)] // Each one is a distinct property of the run.
fn draw_text<S: Surface + ?Sized>(
    surface: &mut S,
    text: &str,
    color: Color,
    at: Rect,
    clip: Rect,
    font: &Font<'_>,
    scale: u8,
    align: Align,
    valign: VAlign,
) {
    let mag = i32::from(scale);
    let cell_w = u32::from(font.cell_w);
    let cell_h = u32::from(font.cell_h);
    let advance = i32::from(font.advance) * mag;
    let mut pen = at.left() + offset(text, at, advance, cell_w as i32 * mag, align);
    // The glyph's own height, not the line's: there is one line, and leading
    // above and below it would only push the text off the centre it was asked
    // to sit on.
    let ink_h = cell_h as i32 * mag;
    let top = at.top()
        + match valign {
            VAlign::Top => 0,
            VAlign::Middle => ((at.size.h as i32 - ink_h) / 2).max(0),
            VAlign::Bottom => (at.size.h as i32 - ink_h).max(0),
        };

    for ch in text.chars() {
        // Stop once the pen is past the clip: everything after it is further
        // right, so no later glyph can land inside.
        if pen >= clip.right() {
            break;
        }
        // A glyph entirely left of the clip still has to advance the pen.
        if pen + cell_w as i32 * mag > clip.left() {
            for y in 0..cell_h {
                let row_y = top + y as i32 * mag;
                // A magnified row is `mag` pixels tall, so the whole band has
                // to be off the clip before it can be skipped.
                if row_y + mag <= clip.top() || row_y >= clip.bottom() {
                    continue;
                }
                let mut x = 0;
                while x < cell_w {
                    if !font.pixel(ch, x, y) {
                        x += 1;
                        continue;
                    }
                    let start = x;
                    while x < cell_w && font.pixel(ch, x, y) {
                        x += 1;
                    }
                    // One rectangle per run of ink, magnified: at scale 12 a
                    // three-pixel run is one 36x12 fill rather than 432 pixels.
                    let run = Rect::new(
                        pen + start as i32 * mag,
                        row_y,
                        (x - start) * u32::from(scale),
                        scale.into(),
                    );
                    fill_clipped(surface, run, clip, color);
                }
            }
        }
        pen += advance;
    }
}

/// Draw a ring segment inside `at`.
///
/// Stepped in angle rather than rasterised as two arcs and filled between
/// them: a step small enough that consecutive points touch produces a solid
/// band with no seam, and the step count follows the radius so a small dial
/// does not pay for a large one's resolution.
///
/// The alternative -- an even-odd fill between an inner and an outer arc --
/// needs a scanline crossing list, which is a lot of machinery for a shape
/// that is only ever a ring.
#[allow(clippy::too_many_arguments)] // Every one is a distinct dial property.
fn arc<S: Surface + ?Sized>(
    surface: &mut S,
    at: Rect,
    clip: Rect,
    start_deg: i32,
    end_deg: i32,
    value: f32,
    thickness: u32,
    fill: Color,
    track: Color,
    antialias: bool,
) {
    use crate::trig::{ONE, TURN, cos, sin};

    let outer = (at.size.w.min(at.size.h) / 2) as i32;
    if outer <= 0 {
        return;
    }
    if antialias {
        // The track over the whole sweep and the fill over the lit part of
        // it, each as one blended ring: no stepping, so no seams either.
        let to_brad = |deg: i32| {
            ((deg as i64 * TURN as i64) / 360 - i64::from(TURN / 4))
                .clamp(i32::MIN as i64, i32::MAX as i64) as i32
        };
        let (a0, a1) = (to_brad(start_deg), to_brad(end_deg));
        let sweep = a1.saturating_sub(a0);
        let c = (
            at.left() as f32 + at.size.w as f32 / 2.0,
            at.top() as f32 + at.size.h as f32 / 2.0,
        );
        let outer = at.size.w.min(at.size.h) as f32 / 2.0;
        let inner = (outer - thickness.max(1) as f32).max(0.0);
        let lit = (sweep as f32 * value) as i32;
        super::aa::arc(surface, clip, c, inner, outer, a0, sweep, track);
        super::aa::arc(surface, clip, c, inner, outer, a0, lit, fill);
        return;
    }
    let inner = (outer - thickness.max(1) as i32).max(0);
    let cx = at.left() + at.size.w as i32 / 2;
    let cy = at.top() + at.size.h as i32 / 2;

    // Degrees clockwise from twelve o'clock, into brads measured the way the
    // table is: a quarter turn back puts zero at the top.
    // Converted in i64 and clamped rather than folded: a scene may name any
    // i32 as an angle and `deg * TURN` overflows long before i32::MAX, but
    // folding each angle on its own turns 0..360 into a zero sweep -- the two
    // ends of a full circle are the same direction and different sweeps.
    // `sin` and `cos` mask internally, so a large brad value is harmless.
    // The quarter-turn offset is applied before the clamp, or clamping to
    // i32::MIN and then subtracting from it overflows.
    let to_brad = |deg: i32| {
        ((deg as i64 * TURN as i64) / 360 - i64::from(TURN / 4))
            .clamp(i32::MIN as i64, i32::MAX as i64) as i32
    };
    let (a0, a1) = (to_brad(start_deg), to_brad(end_deg));
    let sweep = a1.saturating_sub(a0);

    // One step per pixel of outer arc length, so consecutive steps land on
    // adjacent pixels: any coarser leaves gaps, any finer redraws the same
    // pixel. Arc length is r * angle, and TURN brads is 2*pi radians.
    let steps =
        ((outer as i64 * sweep.unsigned_abs() as i64 * 7) / (TURN as i64)).clamp(1, 4096) as i32;
    let lit = (steps as f32 * value) as i32;

    for i in 0..=steps {
        let colour = if i <= lit { fill } else { track };
        if colour.is_transparent() {
            continue;
        }
        // In i64: `sweep * i` overflows for a large sweep long before the
        // division brings it back into range.
        let a = (i64::from(a0) + i64::from(sweep) * i64::from(i) / i64::from(steps))
            .clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        let (s, c) = (sin(a), cos(a));
        // Walk the ring's thickness at this angle. Stepping the radius rather
        // than drawing a line keeps every pixel inside the annulus, which a
        // thick line across the band would not.
        for r in inner..=outer {
            let x = cx + (c * r) / ONE;
            let y = cy + (s * r) / ONE;
            fill_clipped(surface, Rect::new(x, y, 1, 1), clip, colour);
        }
    }
}

/// Clamp to 0.0..=1.0, treating NaN as zero.
///
/// NaN fails every comparison, so a bare `clamp` leaves it NaN and the cast to
/// a pixel count or an alpha becomes implementation-defined.
fn clamp01(v: f32) -> f32 {
    if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }
}

/// Fill `rect` with its corners rounded by `radius`.
///
/// Row by row rather than by drawing four quarter-discs and three rectangles:
/// one span per row is the same shape the square case draws, so a rounded
/// panel costs what a square one does plus an integer square root per corner
/// row. The alternative leaves seams where the pieces meet.
fn round_rect<S: Surface + ?Sized>(
    surface: &mut S,
    at: Rect,
    clip: Rect,
    radius: u32,
    color: Color,
    antialias: bool,
) {
    let (w, h) = (at.size.w, at.size.h);
    // A radius past half the shorter side would make the corner arcs overlap
    // and the middle of the shape vanish.
    let r = radius.min(w / 2).min(h / 2) as i32;
    if r <= 0 {
        fill_clipped(surface, at, clip, color);
        return;
    }
    if antialias {
        // The corner rows are sampled against the four corner discs; the
        // rows between them are the plain spans they always were.
        let Some(inside_clip) = clip.intersection(at) else {
            return;
        };
        let (l, t, rt, b) = (
            at.left() as f32,
            at.top() as f32,
            at.right() as f32,
            at.bottom() as f32,
        );
        let rf = r as f32;
        let inside = |px: f32, py: f32| {
            let cx = if px < l + rf {
                l + rf
            } else if px > rt - rf {
                rt - rf
            } else {
                return true;
            };
            let cy = if py < t + rf {
                t + rf
            } else if py > b - rf {
                b - rf
            } else {
                return true;
            };
            let (vx, vy) = (px - cx, py - cy);
            vx * vx + vy * vy <= rf * rf
        };
        for y in at.top()..at.bottom() {
            if y < at.top() + r || y >= at.bottom() - r {
                super::aa::shade_row(
                    surface,
                    inside_clip,
                    y,
                    at.left(),
                    at.right(),
                    color,
                    &inside,
                );
            } else {
                fill_clipped(surface, Rect::new(at.left(), y, w, 1), clip, color);
            }
        }
        return;
    }

    for y in 0..h as i32 {
        // Distance from whichever end this row is near, or zero in the middle.
        let dy = if y < r {
            r - y
        } else if y >= h as i32 - r {
            y - (h as i32 - r) + 1
        } else {
            0
        };
        let inset = if dy > 0 {
            r - crate::trig::isqrt(r * r - dy * dy)
        } else {
            0
        };
        let span = Rect::new(
            at.left() + inset,
            at.top() + y,
            (w as i32 - 2 * inset).max(0) as u32,
            1,
        );
        fill_clipped(surface, span, clip, color);
    }
}

/// Fill `rect`, but never outside `clip`.
///
/// [`fill_rect`] clips to the *surface*, which is a different and weaker
/// guarantee: a widget drawn during a partial repaint must also stay inside
/// the damage rectangle, or it erases neighbours that were never dirty. Every
/// primitive below goes through here rather than calling `fill_rect` directly.
fn fill_clipped<S: Surface + ?Sized>(surface: &mut S, rect: Rect, clip: Rect, color: Color) {
    if let Some(r) = rect.intersection(clip) {
        fill_rect(surface, r, color);
    }
}

/// Draw one widget into `surface` at `at` (already in screen coordinates).
///
/// `clip` is the damage rectangle being repainted; nothing outside it may be
/// touched.
pub fn draw_kind<S: Surface + ?Sized>(
    surface: &mut S,
    kind: &Kind,
    at: Rect,
    clip: Rect,
    res: Resources<'_>,
    antialias: bool,
) {
    // Early exit if the widget does not overlap the damage region;
    // this avoids any unnecessary rasterisation work.
    let Some(area) = at.intersection(clip) else {
        return;
    };

    if area.is_empty() {
        return;
    }

    match kind {
        Kind::Panel { background } => {
            // Transparent panels contribute nothing to the output.
            if background.is_transparent() {
                return;
            }
            fill_rect(surface, area, *background);
        }
        Kind::Frame { color } => {
            if color.is_transparent() {
                return;
            }
            // The outline is positioned by the widget's own rectangle, but
            // must still be confined to the damage rectangle. When the two
            // coincide the fast path is exact; otherwise the edges are filled
            // individually so each can be clipped.
            if clip.contains_rect(at) {
                stroke_rect(surface, at, *color);
            } else {
                let (w, h) = (at.size.w, at.size.h);
                fill_clipped(surface, Rect::new(at.left(), at.top(), w, 1), clip, *color);
                if h > 1 {
                    let y = at.bottom() - 1;
                    fill_clipped(surface, Rect::new(at.left(), y, w, 1), clip, *color);
                }
                if h > 2 {
                    let (y, ih) = (at.top() + 1, h - 2);
                    fill_clipped(surface, Rect::new(at.left(), y, 1, ih), clip, *color);
                    if w > 1 {
                        let x = at.right() - 1;
                        fill_clipped(surface, Rect::new(x, y, 1, ih), clip, *color);
                    }
                }
            }
        }
        Kind::Label {
            text,
            color,
            scale,
            align,
            valign,
        } => {
            if color.is_transparent() {
                return;
            }
            draw_text(
                surface,
                text,
                *color,
                at,
                clip,
                res.font,
                (*scale).max(1),
                *align,
                *valign,
            );
        }
        Kind::Image { image } => {
            // A widget naming an image the host never supplied draws the
            // magenta placeholder, not nothing: nothing is indistinguishable
            // from a transparent widget that is working correctly.
            let Some(img) = res.images.get(*image) else {
                fill_clipped(surface, at, clip, Color::rgb(255, 0, 255));
                return;
            };
            // Scaling to the widget's rectangle is what lets a scene be
            // authored once and shown on a panel of a different size.
            if img.width == at.size.w && img.height == at.size.h {
                blit(surface, at, &img.pixels, img.width);
            } else {
                let scaled =
                    scale_nearest(&img.pixels, img.width, img.height, at.size.w, at.size.h);
                blit(surface, at, &scaled, at.size.w);
            }
        }
        Kind::Anim { anim, frame, .. } => {
            let Some(a) = res.anims.get(*anim) else {
                fill_clipped(surface, at, clip, Color::rgb(255, 0, 255));
                return;
            };
            // A frame index past the end shows the last frame rather than
            // nothing: a widget seeked out of range should look wrong in an
            // obvious way, not vanish.
            let idx = (*frame as usize).min(a.frames.len().saturating_sub(1));
            let Some(f) = a.frames.get(idx) else { return };
            if u32::from(a.width) == at.size.w && u32::from(a.height) == at.size.h {
                blit(surface, at, &f.pixels, u32::from(a.width));
            } else {
                let scaled = scale_nearest(
                    &f.pixels,
                    u32::from(a.width),
                    u32::from(a.height),
                    at.size.w,
                    at.size.h,
                );
                blit(surface, at, &scaled, at.size.w);
            }
        }
        Kind::Arc {
            start,
            end,
            value,
            thickness,
            fill,
            track,
        } => {
            arc(
                surface,
                at,
                clip,
                *start,
                *end,
                clamp01(*value),
                *thickness,
                *fill,
                *track,
                antialias,
            );
        }
        Kind::Needle {
            start,
            end,
            value,
            width,
            color,
            hub,
        } => {
            super::gauge::needle(
                surface,
                at,
                clip,
                *start,
                *end,
                clamp01(*value),
                *width,
                *color,
                *hub,
                antialias,
            );
        }
        Kind::Scale {
            start,
            end,
            ticks,
            major_every,
            length,
            width,
            color,
            major_color,
        } => {
            super::gauge::scale(
                surface,
                at,
                clip,
                *start,
                *end,
                *ticks,
                *major_every,
                *length,
                *width,
                *color,
                *major_color,
                antialias,
            );
        }
        Kind::Line {
            points,
            width,
            color,
            closed,
        } => {
            super::plot::polyline(
                surface, at, clip, points, *width, *color, *closed, antialias,
            );
        }
        Kind::Polygon { points, color } => {
            super::plot::polygon(surface, at, clip, points, *color, antialias);
        }
        Kind::Chart {
            values,
            width,
            stroke,
            fill,
        } => {
            super::plot::chart(surface, at, clip, values, *width, *stroke, *fill, antialias);
        }
        Kind::SevenSeg {
            text,
            color,
            ghost,
            thickness,
            digits,
        } => {
            super::segment::seven_seg(surface, at, clip, text, *color, *ghost, *thickness, *digits);
        }
        Kind::Gradient { from, to, vertical } => {
            super::gradient::gradient(surface, at, clip, *from, *to, *vertical);
        }
        Kind::Ruler {
            ticks,
            major_every,
            length,
            width,
            color,
            major_color,
            vertical,
        } => {
            super::gauge::ruler(
                surface,
                at,
                clip,
                *ticks,
                *major_every,
                *length,
                *width,
                *color,
                *major_color,
                *vertical,
            );
        }
        Kind::SegBar {
            value,
            segments,
            gap,
            fill,
            track,
            warn,
            warn_fill,
            danger,
            danger_fill,
            vertical,
            profile,
            height,
            divisions,
            div_gap,
        } => {
            super::segbar::seg_bar(
                surface,
                at,
                clip,
                clamp01(*value),
                clamp01(*height),
                *segments,
                *gap,
                *fill,
                *track,
                *warn,
                *warn_fill,
                *danger,
                *danger_fill,
                *vertical,
                profile,
                *divisions,
                *div_gap,
                antialias,
            );
        }
        Kind::Grid {
            pitch_x,
            pitch_y,
            width,
            color,
        } => {
            super::grid::grid(surface, at, clip, *pitch_x, *pitch_y, *width, *color);
        }
        Kind::Led { color, level, glow } => {
            // A lamp is drawn even when dark, at its glow level: a hole where
            // a symbol should be tells the driver nothing, while a faint one
            // tells them the lamp exists and is currently off.
            let lit = clamp01(*level);
            let dim = clamp01(*glow);
            let mix = dim + (1.0 - dim) * lit;
            // Scaling to zero would paint opaque black, which is a lamp-shaped
            // hole rather than nothing. `glow: 0` means invisible when off.
            if mix <= 0.0 {
                return;
            }
            // The components are scaled, not the alpha. A surface fill
            // overwrites rather than blending, and a framebuffer has no alpha
            // channel to blend against anyway, so a dimmed lamp has to be a
            // darker colour rather than a more transparent one.
            let scale = |v: u8| (f32::from(v) * mix) as u8;
            let c = Color::rgba(scale(color.r), scale(color.g), scale(color.b), color.a);
            if !c.is_transparent() {
                fill_clipped(surface, at, clip, c);
            }
        }
        Kind::RoundRect { background, radius } => {
            if background.is_transparent() {
                return;
            }
            round_rect(surface, at, clip, *radius, *background, antialias);
        }
        Kind::Bar {
            value,
            fill,
            track,
            vertical,
        } => {
            // Clamp value into [0.0, 1.0]; NaN is treated as 0.0 to avoid
            // undefined geometry.
            let v = if value.is_nan() {
                0.0
            } else {
                value.clamp(0.0, 1.0)
            };

            // Draw the track first so the fill overlays it.
            if !track.is_transparent() {
                fill_clipped(surface, at, clip, *track);
            }

            if fill.is_transparent() {
                return;
            }

            if antialias {
                // The leading edge lands where the value puts it, to a
                // fraction of a pixel, so a slow sweep moves smoothly rather
                // than one whole pixel at a time.
                let (l, t, r, b) = (
                    at.left() as f32,
                    at.top() as f32,
                    at.right() as f32,
                    at.bottom() as f32,
                );
                if *vertical {
                    let top = b - at.size.h as f32 * v;
                    super::aa::frac_rect(surface, clip, l, top, r, b, *fill, true);
                } else {
                    let right = l + at.size.w as f32 * v;
                    super::aa::frac_rect(surface, clip, l, t, right, b, *fill, true);
                }
                return;
            }

            if *vertical {
                // Vertical bars grow upward from the bottom edge.
                let height = (at.size.h as f32 * v) as u32;
                if height > 0 {
                    let filled =
                        Rect::new(at.left(), at.bottom() - height as i32, at.size.w, height);
                    fill_clipped(surface, filled, clip, *fill);
                }
            } else {
                // Horizontal bars grow rightward from the left edge.
                let width = (at.size.w as f32 * v) as u32;
                if width > 0 {
                    let filled = Rect::new(at.left(), at.top(), width, at.size.h);
                    fill_clipped(surface, filled, clip, *fill);
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    use crate::{MemorySurface, PixelFormat, Size};

    pub fn surf(w: u32, h: u32) -> MemorySurface {
        MemorySurface::new(Size { w, h }, PixelFormat::Bgrx8888)
    }

    pub fn is_set(s: &MemorySurface, x: usize, y: usize) -> bool {
        let o = y * s.stride() + x * 4;
        s.pixels()[o..o + 4] != [0, 0, 0, 0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::{AnimTable, Image, ImageTable};
    use crate::font::{Font, default_font};
    use crate::widget::{Align, Kind, VAlign};
    use crate::{Color, MemorySurface, PixelFormat, Rect, Size};
    use alloc::string::String;

    /// Parsed once per call; the atlas is 683 bytes and parsing is a
    /// header read, so a test does not need to cache it.
    fn font() -> Font<'static> {
        default_font()
    }

    /// Empty tables plus the built-in font, which is what most tests need.
    fn res<'a>(images: &'a ImageTable, anims: &'a AnimTable, font: &'a Font<'a>) -> Resources<'a> {
        Resources {
            images,
            anims,
            font,
        }
    }

    fn surf(w: u32, h: u32) -> MemorySurface {
        MemorySurface::new(Size { w, h }, PixelFormat::Bgrx8888)
    }

    /// Pixel at (x, y) as (b, g, r, x).
    fn px(s: &MemorySurface, x: usize, y: usize) -> [u8; 4] {
        let o = y * s.stride() + x * 4;
        [
            s.pixels()[o],
            s.pixels()[o + 1],
            s.pixels()[o + 2],
            s.pixels()[o + 3],
        ]
    }

    fn is_set(s: &MemorySurface, x: usize, y: usize) -> bool {
        px(s, x, y) != [0, 0, 0, 0]
    }

    fn count_set(s: &MemorySurface) -> usize {
        let w = s.stride() / 4;
        let h = s.pixels().len() / s.stride();
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .filter(|&(x, y)| is_set(s, x, y))
            .count()
    }

    const FULL: Rect = Rect::new(0, 0, 100, 100);

    // --- panel ---

    #[test]
    fn a_panel_fills_its_rectangle() {
        let mut s = surf(10, 10);
        let k = Kind::Panel {
            background: Color::WHITE,
        };
        draw_kind(
            &mut s,
            &k,
            Rect::new(2, 3, 4, 2),
            FULL,
            res(&ImageTable::new(), &AnimTable::new(), &font()),
            false,
        );
        assert_eq!(count_set(&s), 8);
        assert!(is_set(&s, 2, 3) && is_set(&s, 5, 4));
        assert!(!is_set(&s, 1, 3) && !is_set(&s, 6, 3));
    }

    #[test]
    fn a_transparent_panel_draws_nothing() {
        let mut s = surf(10, 10);
        let k = Kind::Panel {
            background: Color::TRANSPARENT,
        };
        draw_kind(
            &mut s,
            &k,
            Rect::new(0, 0, 10, 10),
            FULL,
            res(&ImageTable::new(), &AnimTable::new(), &font()),
            false,
        );
        assert_eq!(count_set(&s), 0);
    }

    #[test]
    fn nothing_outside_the_clip_is_touched() {
        // The compositor's whole contract: a repaint of one damage rectangle
        // must not disturb a widget that was not dirty.
        let mut s = surf(10, 10);
        let k = Kind::Panel {
            background: Color::WHITE,
        };
        draw_kind(
            &mut s,
            &k,
            Rect::new(0, 0, 10, 10),
            Rect::new(0, 0, 3, 3),
            res(&ImageTable::new(), &AnimTable::new(), &font()),
            false,
        );
        assert_eq!(count_set(&s), 9);
        assert!(is_set(&s, 2, 2));
        assert!(!is_set(&s, 3, 0) && !is_set(&s, 0, 3));
    }

    #[test]
    fn a_widget_disjoint_from_the_clip_draws_nothing() {
        let mut s = surf(10, 10);
        let k = Kind::Panel {
            background: Color::WHITE,
        };
        draw_kind(
            &mut s,
            &k,
            Rect::new(0, 0, 4, 4),
            Rect::new(5, 5, 4, 4),
            res(&ImageTable::new(), &AnimTable::new(), &font()),
            false,
        );
        assert_eq!(count_set(&s), 0);
    }

    // --- frame ---

    #[test]
    fn a_frame_draws_only_its_outline() {
        let mut s = surf(10, 10);
        let k = Kind::Frame {
            color: Color::WHITE,
        };
        draw_kind(
            &mut s,
            &k,
            Rect::new(0, 0, 4, 4),
            FULL,
            res(&ImageTable::new(), &AnimTable::new(), &font()),
            false,
        );
        // Perimeter of a 4x4 box, corners counted once: 12.
        assert_eq!(count_set(&s), 12);
        assert!(is_set(&s, 0, 0) && is_set(&s, 3, 3));
        assert!(!is_set(&s, 1, 1), "the interior must stay clear");
    }

    #[test]
    fn a_transparent_frame_draws_nothing() {
        let mut s = surf(10, 10);
        let k = Kind::Frame {
            color: Color::TRANSPARENT,
        };
        draw_kind(
            &mut s,
            &k,
            Rect::new(0, 0, 4, 4),
            FULL,
            res(&ImageTable::new(), &AnimTable::new(), &font()),
            false,
        );
        assert_eq!(count_set(&s), 0);
    }

    // --- not implemented yet ---

    #[test]
    fn a_label_draws_its_text() {
        let mut s = surf(40, 10);
        let k = Kind::Label {
            text: String::from("88"),
            color: Color::WHITE,
            scale: 1,
            align: Align::Left,
            valign: VAlign::Top,
        };
        draw_kind(
            &mut s,
            &k,
            Rect::new(0, 0, 40, 10),
            FULL,
            res(&ImageTable::new(), &AnimTable::new(), &font()),
            false,
        );
        assert!(count_set(&s) > 20, "expected glyph ink");
        assert!(is_set(&s, 1, 0), "first glyph");
        assert!(is_set(&s, 7, 0), "second glyph, one advance along");
    }

    #[test]
    fn an_empty_label_draws_nothing() {
        let mut s = surf(40, 10);
        let k = Kind::Label {
            text: String::new(),
            color: Color::WHITE,
            scale: 1,
            align: Align::Left,
            valign: VAlign::Top,
        };
        draw_kind(
            &mut s,
            &k,
            Rect::new(0, 0, 40, 10),
            FULL,
            res(&ImageTable::new(), &AnimTable::new(), &font()),
            false,
        );
        assert_eq!(count_set(&s), 0);
    }

    #[test]
    fn a_transparent_label_draws_nothing() {
        let mut s = surf(40, 10);
        let k = Kind::Label {
            text: String::from("88"),
            color: Color::TRANSPARENT,
            scale: 1,
            align: Align::Left,
            valign: VAlign::Top,
        };
        draw_kind(
            &mut s,
            &k,
            Rect::new(0, 0, 40, 10),
            FULL,
            res(&ImageTable::new(), &AnimTable::new(), &font()),
            false,
        );
        assert_eq!(count_set(&s), 0);
    }

    #[test]
    fn a_label_is_clipped_rather_than_wrapped() {
        // A label is a fixed box in a scene; reflowing it would move things
        // the author positioned by hand.
        let mut s = surf(40, 10);
        let k = Kind::Label {
            text: String::from("8888888888"),
            color: Color::WHITE,
            scale: 1,
            align: Align::Left,
            valign: VAlign::Top,
        };
        draw_kind(
            &mut s,
            &k,
            Rect::new(0, 0, 40, 10),
            Rect::new(0, 0, 12, 10),
            res(&ImageTable::new(), &AnimTable::new(), &font()),
            false,
        );
        for y in 0..10 {
            for x in 12..40 {
                assert!(!is_set(&s, x, y), "ink escaped the clip at {x},{y}");
            }
        }
    }

    #[test]
    fn a_label_starting_left_of_the_clip_still_lands_correctly() {
        // The pen has to advance through glyphs it does not draw, or the text
        // shifts left by however many were skipped.
        let mut a = surf(40, 10);
        let mut b = surf(40, 10);
        let k = Kind::Label {
            text: String::from("18"),
            color: Color::WHITE,
            scale: 1,
            align: Align::Left,
            valign: VAlign::Top,
        };
        draw_kind(
            &mut a,
            &k,
            Rect::new(0, 0, 40, 10),
            FULL,
            res(&ImageTable::new(), &AnimTable::new(), &font()),
            false,
        );
        draw_kind(
            &mut b,
            &k,
            Rect::new(0, 0, 40, 10),
            Rect::new(6, 0, 34, 10),
            res(&ImageTable::new(), &AnimTable::new(), &font()),
            false,
        );
        for y in 0..10 {
            for x in 6..40 {
                assert_eq!(
                    is_set(&a, x, y),
                    is_set(&b, x, y),
                    "second glyph moved at {x},{y}"
                );
            }
        }
    }

    #[test]
    fn an_image_with_no_table_entry_draws_the_missing_placeholder() {
        // Drawing nothing would be indistinguishable from a widget that works
        // correctly and is simply transparent -- the worst possible outcome
        // for someone debugging a wrong asset path.
        let mut s = surf(10, 10);
        draw_kind(
            &mut s,
            &Kind::Image { image: 0 },
            Rect::new(0, 0, 4, 4),
            FULL,
            res(&ImageTable::new(), &AnimTable::new(), &font()),
            false,
        );
        assert_eq!(count_set(&s), 16);
        assert_eq!(px(&s, 0, 0), [255, 0, 255, 255], "magenta placeholder");
    }

    #[test]
    fn an_image_is_blitted_at_its_natural_size() {
        let mut s = surf(10, 10);
        let mut table = ImageTable::new();
        table.push(Image::new(2, 2, alloc::vec![Color::rgb(0, 255, 0); 4]).unwrap());
        draw_kind(
            &mut s,
            &Kind::Image { image: 0 },
            Rect::new(1, 1, 2, 2),
            FULL,
            res(&table, &AnimTable::new(), &font()),
            false,
        );
        assert_eq!(count_set(&s), 4);
        assert_eq!(px(&s, 1, 1), [0, 255, 0, 255]);
    }

    #[test]
    fn an_image_is_scaled_to_the_widget_rectangle() {
        // A scene authored for one panel has to render on another.
        let mut s = surf(10, 10);
        let mut table = ImageTable::new();
        table.push(Image::new(1, 1, alloc::vec![Color::rgb(0, 0, 255)]).unwrap());
        draw_kind(
            &mut s,
            &Kind::Image { image: 0 },
            Rect::new(0, 0, 6, 6),
            FULL,
            res(&table, &AnimTable::new(), &font()),
            false,
        );
        assert_eq!(count_set(&s), 36);
    }

    // --- bar ---

    fn bar(value: f32, vertical: bool) -> Kind {
        Kind::Bar {
            value,
            fill: Color::WHITE,
            track: Color::TRANSPARENT,
            vertical,
        }
    }

    #[test]
    fn a_horizontal_bar_fills_from_the_left() {
        let mut s = surf(10, 10);
        draw_kind(
            &mut s,
            &bar(0.5, false),
            Rect::new(0, 0, 10, 2),
            FULL,
            res(&ImageTable::new(), &AnimTable::new(), &font()),
            false,
        );
        assert_eq!(count_set(&s), 10, "half of 10 wide, 2 tall");
        assert!(is_set(&s, 4, 0));
        assert!(!is_set(&s, 5, 0), "the right half must stay clear");
    }

    #[test]
    fn a_vertical_bar_grows_upward_from_the_bottom() {
        // A fuel gauge that filled downward would read exactly backwards.
        let mut s = surf(10, 10);
        draw_kind(
            &mut s,
            &bar(0.5, true),
            Rect::new(0, 0, 2, 10),
            FULL,
            res(&ImageTable::new(), &AnimTable::new(), &font()),
            false,
        );
        assert_eq!(count_set(&s), 10);
        assert!(is_set(&s, 0, 9), "the bottom row must be filled");
        assert!(!is_set(&s, 0, 4), "the top half must stay clear");
    }

    #[test]
    fn a_full_bar_covers_its_whole_rectangle() {
        let mut s = surf(10, 10);
        draw_kind(
            &mut s,
            &bar(1.0, false),
            Rect::new(0, 0, 10, 10),
            FULL,
            res(&ImageTable::new(), &AnimTable::new(), &font()),
            false,
        );
        assert_eq!(count_set(&s), 100);
    }

    #[test]
    fn an_empty_bar_draws_no_fill() {
        let mut s = surf(10, 10);
        draw_kind(
            &mut s,
            &bar(0.0, false),
            Rect::new(0, 0, 10, 10),
            FULL,
            res(&ImageTable::new(), &AnimTable::new(), &font()),
            false,
        );
        assert_eq!(count_set(&s), 0);
    }

    #[test]
    fn a_bar_value_out_of_range_is_clamped_not_wrapped() {
        let mut s = surf(10, 10);
        draw_kind(
            &mut s,
            &bar(9.0, false),
            Rect::new(0, 0, 10, 10),
            FULL,
            res(&ImageTable::new(), &AnimTable::new(), &font()),
            false,
        );
        assert_eq!(count_set(&s), 100);

        let mut s = surf(10, 10);
        draw_kind(
            &mut s,
            &bar(-9.0, false),
            Rect::new(0, 0, 10, 10),
            FULL,
            res(&ImageTable::new(), &AnimTable::new(), &font()),
            false,
        );
        assert_eq!(count_set(&s), 0);
    }

    #[test]
    fn a_nan_bar_value_is_treated_as_empty_rather_than_panicking() {
        // NaN fails every comparison, so a naive clamp leaves it NaN and the
        // cast to a pixel count is then implementation-defined.
        let mut s = surf(10, 10);
        draw_kind(
            &mut s,
            &bar(f32::NAN, false),
            Rect::new(0, 0, 10, 10),
            FULL,
            res(&ImageTable::new(), &AnimTable::new(), &font()),
            false,
        );
        assert_eq!(count_set(&s), 0);
    }

    #[test]
    fn a_bar_track_is_drawn_under_the_fill() {
        let mut s = surf(10, 10);
        let k = Kind::Bar {
            value: 0.5,
            fill: Color::rgb(255, 0, 0),
            track: Color::rgb(0, 0, 255),
            vertical: false,
        };
        draw_kind(
            &mut s,
            &k,
            Rect::new(0, 0, 10, 1),
            FULL,
            res(&ImageTable::new(), &AnimTable::new(), &font()),
            false,
        );
        assert_eq!(px(&s, 0, 0), [0, 0, 255, 255], "left half is the fill");
        assert_eq!(px(&s, 9, 0), [255, 0, 0, 255], "right half is the track");
    }

    #[test]
    fn a_bar_clipped_to_a_damage_rect_stays_inside_it() {
        let mut s = surf(10, 10);
        draw_kind(
            &mut s,
            &bar(1.0, false),
            Rect::new(0, 0, 10, 10),
            Rect::new(0, 0, 2, 2),
            res(&ImageTable::new(), &AnimTable::new(), &font()),
            false,
        );
        assert_eq!(count_set(&s), 4);
    }

    // --- label alignment ---

    /// Draw `text` in a `w`-wide box and report the first and last lit column.
    fn ink(text: &str, w: u32, align: Align) -> Option<(usize, usize)> {
        let mut s = surf(w, 16);
        let at = Rect::new(0, 0, w, 16);
        let k = Kind::Label {
            text: String::from(text),
            color: Color::WHITE,
            scale: 1,
            align,
            valign: VAlign::Top,
        };
        let (images, anims, font) = (ImageTable::new(), AnimTable::new(), font());
        draw_kind(&mut s, &k, at, at, res(&images, &anims, &font), false);
        let lit: alloc::vec::Vec<usize> = (0..w as usize)
            .filter(|&x| (0..16).any(|y| is_set(&s, x, y)))
            .collect();
        Some((*lit.first()?, *lit.last()?))
    }

    /// Draw `text` in a box `h` tall and report the first and last lit row.
    fn rows(text: &str, h: u32, valign: VAlign) -> Option<(usize, usize)> {
        let mut s = surf(64, h);
        let at = Rect::new(0, 0, 64, h);
        let k = Kind::Label {
            text: String::from(text),
            color: Color::WHITE,
            scale: 1,
            align: Align::Left,
            valign,
        };
        let (images, anims, font) = (ImageTable::new(), AnimTable::new(), font());
        draw_kind(&mut s, &k, at, at, res(&images, &anims, &font), false);
        let lit: alloc::vec::Vec<usize> = (0..h as usize)
            .filter(|&y| (0..64).any(|x| is_set(&s, x, y)))
            .collect();
        Some((*lit.first()?, *lit.last()?))
    }

    #[test]
    fn a_top_aligned_label_starts_at_the_top_edge() {
        let (first, _) = rows("HI", 40, VAlign::Top).expect("some ink");
        assert!(first < 2, "started at row {first}");
    }

    #[test]
    fn a_bottom_aligned_label_ends_at_the_bottom_edge() {
        let (_, last) = rows("HI", 40, VAlign::Bottom).expect("some ink");
        assert!(last >= 38, "ended at row {last}");
    }

    #[test]
    fn a_middle_aligned_label_has_equal_slack_above_and_below() {
        let h = 40usize;
        let (first, last) = rows("HI", h as u32, VAlign::Middle).expect("some ink");
        let above = first;
        let below = h - 1 - last;
        // The glyph cell is taller than its ink, so allow the descender gap;
        // what matters is that the two margins agree with each other.
        assert!(
            above.abs_diff(below) <= 2,
            "slack {above} above against {below} below"
        );
    }

    #[test]
    fn vertical_alignment_does_not_change_how_tall_the_text_is() {
        let heights: alloc::vec::Vec<usize> = [VAlign::Top, VAlign::Middle, VAlign::Bottom]
            .into_iter()
            .map(|v| {
                let (f, l) = rows("HI", 40, v).expect("some ink");
                l - f
            })
            .collect();
        assert_eq!(heights[0], heights[1]);
        assert_eq!(heights[1], heights[2]);
    }

    #[test]
    fn text_taller_than_its_box_still_starts_at_the_top() {
        // Centring would push the top of the glyphs off the edge, and the top
        // is the half that makes a letter recognisable.
        for v in [VAlign::Middle, VAlign::Bottom] {
            let (first, _) = rows("HI", 4, v).expect("some ink");
            assert!(first < 2, "{v:?} started at row {first}");
        }
    }

    #[test]
    fn a_left_aligned_label_starts_at_the_left_edge() {
        let (first, _) = ink("HI", 64, Align::Left).expect("some ink");
        assert!(first < 2, "started at column {first}");
    }

    #[test]
    fn a_right_aligned_label_ends_at_the_right_edge() {
        let (_, last) = ink("HI", 64, Align::Right).expect("some ink");
        assert!(last >= 62, "ended at column {last}");
    }

    #[test]
    fn a_centred_label_has_equal_slack_on_both_sides() {
        // "HH" rather than a narrower glyph: alignment centres the font's
        // fixed cells, and a letter whose ink stops short of its cell edge
        // would make the measurement disagree with the intent by that gap.
        let w = 64usize;
        let (first, last) = ink("HH", w as u32, Align::Center).expect("some ink");
        let left = first;
        let right = w - 1 - last;
        // Odd slack cannot split evenly, so one pixel of difference is exact.
        assert!(
            left.abs_diff(right) <= 1,
            "slack {left} on the left against {right} on the right"
        );
    }

    #[test]
    fn alignment_does_not_change_how_wide_the_text_is() {
        let widths: alloc::vec::Vec<usize> = [Align::Left, Align::Center, Align::Right]
            .into_iter()
            .map(|a| {
                let (f, l) = ink("HELLO", 100, a).expect("some ink");
                l - f
            })
            .collect();
        assert_eq!(widths[0], widths[1]);
        assert_eq!(widths[1], widths[2]);
    }

    #[test]
    fn text_too_wide_for_its_box_still_starts_at_the_left() {
        // Centring would otherwise push the opening characters off the left
        // edge, losing the half a reader scans first.
        for align in [Align::Center, Align::Right] {
            let (first, _) = ink("MUCH TOO LONG FOR THIS", 20, align).expect("some ink");
            assert!(first < 2, "{align:?} started at column {first}");
        }
    }

    #[test]
    fn an_empty_label_draws_nothing_whatever_its_alignment() {
        for align in [Align::Left, Align::Center, Align::Right] {
            assert!(ink("", 64, align).is_none(), "{align:?} drew something");
        }
    }
}

#[cfg(test)]
mod clip_tests {
    use super::tests_support::*;
    use super::*;
    use crate::Color;
    use crate::asset::{AnimTable, ImageTable};
    use crate::font::{Font, default_font};

    fn font() -> Font<'static> {
        default_font()
    }

    /// Empty tables plus the built-in font, which is what most tests need.
    fn res<'a>(images: &'a ImageTable, anims: &'a AnimTable, font: &'a Font<'a>) -> Resources<'a> {
        Resources {
            images,
            anims,
            font,
        }
    }

    #[test]
    fn a_frame_partly_outside_the_clip_stays_inside_it() {
        // The latent twin of the bar bug: stroke_rect clips to the surface,
        // not to the damage rectangle, so a frame drawn during a partial
        // repaint would erase neighbours that were never dirty.
        let mut s = surf(10, 10);
        let k = Kind::Frame {
            color: Color::WHITE,
        };
        draw_kind(
            &mut s,
            &k,
            Rect::new(0, 0, 10, 10),
            Rect::new(0, 0, 3, 3),
            res(&ImageTable::new(), &AnimTable::new(), &font()),
            false,
        );
        assert!(is_set(&s, 0, 0));
        assert!(!is_set(&s, 9, 0), "the far edge is outside the clip");
        assert!(!is_set(&s, 0, 9));
    }
}

#[cfg(test)]
mod widget_tests {
    use super::*;

    pub(crate) fn surf64() -> MemorySurface {
        MemorySurface::new(Size { w: 64, h: 64 }, PixelFormat::Bgrx8888)
    }

    pub(crate) fn lit64(s: &MemorySurface, x: usize, y: usize) -> bool {
        let o = y * s.stride() + x * 4;
        s.pixels()[o..o + 4] != [0, 0, 0, 0]
    }

    pub(crate) fn count64(s: &MemorySurface) -> usize {
        (0..64)
            .flat_map(|y| (0..64).map(move |x| (x, y)))
            .filter(|&(x, y)| lit64(s, x, y))
            .count()
    }

    pub(crate) fn draw64(s: &mut MemorySurface, k: &crate::widget::Kind, at: Rect) {
        let (i, a, f) = (
            crate::asset::ImageTable::new(),
            crate::asset::AnimTable::new(),
            crate::font::default_font(),
        );
        draw_kind(
            s,
            k,
            at,
            Rect::new(0, 0, 64, 64),
            Resources {
                images: &i,
                anims: &a,
                font: &f,
            },
            false,
        );
    }

    use crate::asset::{AnimTable, ImageTable};
    use crate::font::{Font, default_font};
    use crate::widget::Kind;
    use crate::{Color, MemorySurface, PixelFormat, Rect, Size};

    fn font() -> Font<'static> {
        default_font()
    }

    fn res<'a>(i: &'a ImageTable, a: &'a AnimTable, f: &'a Font<'a>) -> Resources<'a> {
        Resources {
            images: i,
            anims: a,
            font: f,
        }
    }

    fn surf(w: u32, h: u32) -> MemorySurface {
        MemorySurface::new(Size { w, h }, PixelFormat::Bgrx8888)
    }

    fn px(s: &MemorySurface, x: usize, y: usize) -> [u8; 4] {
        let o = y * s.stride() + x * 4;
        s.pixels()[o..o + 4].try_into().unwrap()
    }

    fn lit(s: &MemorySurface, x: usize, y: usize) -> bool {
        px(s, x, y) != [0, 0, 0, 0]
    }

    fn count(s: &MemorySurface, w: usize, h: usize) -> usize {
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .filter(|&(x, y)| lit(s, x, y))
            .count()
    }

    fn draw(s: &mut MemorySurface, k: &Kind, at: Rect) {
        let (i, a, f) = (ImageTable::new(), AnimTable::new(), font());
        draw_kind(s, k, at, Rect::new(0, 0, 64, 64), res(&i, &a, &f), false);
    }

    // --- led ---

    #[test]
    fn an_unlit_led_still_shows_faintly() {
        // A hole where a symbol should be tells the driver nothing. A faint
        // lamp tells them it exists and is currently off.
        let mut s = surf(16, 16);
        draw(
            &mut s,
            &Kind::Led {
                color: Color::rgb(255, 0, 0),
                level: 0.0,
                glow: 0.3,
            },
            Rect::new(2, 2, 8, 8),
        );
        assert!(lit(&s, 4, 4), "an unlit lamp drew nothing at all");
    }

    #[test]
    fn a_lit_led_is_brighter_than_an_unlit_one() {
        let mut dark = surf(16, 16);
        let mut bright = surf(16, 16);
        for (s, level) in [(&mut dark, 0.0f32), (&mut bright, 1.0)] {
            draw(
                s,
                &Kind::Led {
                    color: Color::rgb(255, 0, 0),
                    level,
                    glow: 0.2,
                },
                Rect::new(0, 0, 8, 8),
            );
        }
        assert!(px(&bright, 2, 2)[2] > px(&dark, 2, 2)[2], "no difference");
    }

    #[test]
    fn a_led_with_no_glow_and_no_level_draws_nothing() {
        let mut s = surf(16, 16);
        draw(
            &mut s,
            &Kind::Led {
                color: Color::WHITE,
                level: 0.0,
                glow: 0.0,
            },
            Rect::new(0, 0, 8, 8),
        );
        assert_eq!(count(&s, 16, 16), 0);
    }

    #[test]
    fn a_nan_level_is_treated_as_off_rather_than_drawn_arbitrarily() {
        let mut s = surf(16, 16);
        draw(
            &mut s,
            &Kind::Led {
                color: Color::WHITE,
                level: f32::NAN,
                glow: 0.0,
            },
            Rect::new(0, 0, 8, 8),
        );
        assert_eq!(count(&s, 16, 16), 0);
    }

    // --- roundrect ---

    #[test]
    fn a_roundrect_fills_its_middle_and_cuts_its_corners() {
        let mut s = surf(32, 32);
        draw(
            &mut s,
            &Kind::RoundRect {
                background: Color::WHITE,
                radius: 6,
            },
            Rect::new(0, 0, 20, 20),
        );
        assert!(lit(&s, 10, 10), "the middle should be filled");
        assert!(lit(&s, 10, 0), "the top edge should be filled");
        assert!(!lit(&s, 0, 0), "the corner should be cut");
        assert!(!lit(&s, 19, 19), "every corner should be cut");
    }

    #[test]
    fn a_zero_radius_is_a_plain_rectangle() {
        let mut a = surf(32, 32);
        let mut b = surf(32, 32);
        draw(
            &mut a,
            &Kind::RoundRect {
                background: Color::WHITE,
                radius: 0,
            },
            Rect::new(0, 0, 10, 10),
        );
        draw(
            &mut b,
            &Kind::Panel {
                background: Color::WHITE,
            },
            Rect::new(0, 0, 10, 10),
        );
        assert_eq!(a.pixels(), b.pixels(), "radius 0 should match a panel");
    }

    #[test]
    fn an_absurd_radius_is_clamped_rather_than_erasing_the_shape() {
        // A radius past half the shorter side would make the corner arcs
        // overlap and the middle vanish.
        let mut s = surf(32, 32);
        draw(
            &mut s,
            &Kind::RoundRect {
                background: Color::WHITE,
                radius: 9999,
            },
            Rect::new(0, 0, 20, 20),
        );
        assert!(lit(&s, 10, 10), "the middle vanished");
        // Clamped to half, so it is a disc: the corners are still cut.
        assert!(!lit(&s, 0, 0));
    }

    #[test]
    fn a_roundrect_is_symmetric_in_both_axes() {
        let mut s = surf(32, 32);
        draw(
            &mut s,
            &Kind::RoundRect {
                background: Color::WHITE,
                radius: 5,
            },
            Rect::new(0, 0, 20, 20),
        );
        for y in 0..20 {
            for x in 0..20 {
                assert_eq!(lit(&s, x, y), lit(&s, 19 - x, y), "x mirror at {x},{y}");
                assert_eq!(lit(&s, x, y), lit(&s, x, 19 - y), "y mirror at {x},{y}");
            }
        }
    }

    #[test]
    fn a_roundrect_covers_less_than_its_bounding_box() {
        let mut s = surf(32, 32);
        draw(
            &mut s,
            &Kind::RoundRect {
                background: Color::WHITE,
                radius: 6,
            },
            Rect::new(0, 0, 20, 20),
        );
        let n = count(&s, 32, 32);
        assert!(n < 400, "nothing was rounded: {n} of 400");
        assert!(n > 300, "too much was cut away: {n} of 400");
    }
}

#[cfg(test)]
mod arc_tests {
    use super::widget_tests::*;

    use crate::widget::Kind;
    use crate::{Color, Rect};

    fn ring(value: f32, thickness: u32) -> Kind {
        Kind::Arc {
            start: 0,
            end: 360,
            value,
            thickness,
            fill: Color::WHITE,
            track: Color::TRANSPARENT,
        }
    }

    #[test]
    fn a_full_ring_is_hollow() {
        // The thing that makes it a ring and not a disc.
        let mut s = surf64();
        draw64(&mut s, &ring(1.0, 6), Rect::new(0, 0, 60, 60));
        assert!(!lit64(&s, 30, 30), "the centre should be clear");
        assert!(lit64(&s, 30, 1), "the top of the ring should be drawn");
    }

    #[test]
    fn a_full_ring_has_no_gaps_around_its_circumference() {
        // Stepping the angle too coarsely leaves a dotted ring, which looks
        // like a rendering fault rather than a design choice.
        let mut s = surf64();
        draw64(&mut s, &ring(1.0, 4), Rect::new(0, 0, 60, 60));
        let mut found = 0;
        for deg in 0..360 {
            let a = (deg as f64).to_radians();
            let r = 27.0;
            let x = (30.0 + r * a.sin()) as usize;
            let y = (30.0 - r * a.cos()) as usize;
            // Accept a hit within one pixel: the ring has thickness, and
            // exact float positions will not land on the same pixel.
            if (x.saturating_sub(1)..=x + 1)
                .any(|px| (y.saturating_sub(1)..=y + 1).any(|py| lit64(&s, px.min(63), py.min(63))))
            {
                found += 1;
            }
        }
        assert!(found > 340, "only {found} of 360 directions had ink");
    }

    #[test]
    fn a_half_filled_arc_paints_about_half() {
        let mut full = surf64();
        let mut half = surf64();
        draw64(&mut full, &ring(1.0, 6), Rect::new(0, 0, 60, 60));
        draw64(&mut half, &ring(0.5, 6), Rect::new(0, 0, 60, 60));
        let (f, h) = (count64(&full), count64(&half));
        assert!(
            h > f / 3 && h < f * 2 / 3,
            "half drew {h} against a full {f}"
        );
    }

    #[test]
    fn an_empty_arc_with_no_track_draws_nothing() {
        let mut s = surf64();
        draw64(&mut s, &ring(0.0, 6), Rect::new(0, 0, 60, 60));
        // Step zero is still drawn as the first lit step, so allow a few.
        assert!(count64(&s) < 20, "an empty arc drew {}", count64(&s));
    }

    #[test]
    fn a_track_paints_the_unfilled_remainder() {
        let mut s = surf64();
        draw64(
            &mut s,
            &Kind::Arc {
                start: 0,
                end: 360,
                value: 0.0,
                thickness: 6,
                fill: Color::WHITE,
                track: Color::rgb(0, 0, 255),
            },
            Rect::new(0, 0, 60, 60),
        );
        assert!(count64(&s) > 200, "the track was not drawn");
    }

    #[test]
    fn a_thicker_ring_uses_more_pixels() {
        let mut thin = surf64();
        let mut thick = surf64();
        draw64(&mut thin, &ring(1.0, 2), Rect::new(0, 0, 60, 60));
        draw64(&mut thick, &ring(1.0, 10), Rect::new(0, 0, 60, 60));
        assert!(count64(&thick) > count64(&thin) * 2);
    }

    #[test]
    fn a_degenerate_arc_draws_nothing_rather_than_panicking() {
        let mut s = surf64();
        for at in [
            Rect::new(0, 0, 0, 0),
            Rect::new(0, 0, 1, 1),
            Rect::new(-100, -100, 4, 4),
        ] {
            draw64(&mut s, &ring(1.0, 4), at);
        }
        draw64(
            &mut s,
            &Kind::Arc {
                start: i32::MAX / 2,
                end: i32::MIN / 2,
                value: 0.5,
                thickness: 9999,
                fill: Color::WHITE,
                track: Color::TRANSPARENT,
            },
            Rect::new(0, 0, 60, 60),
        );
    }

    #[test]
    fn zero_starts_at_twelve_oclock() {
        // A dial that starts somewhere other than where the scene says is the
        // hardest kind of layout bug to see, because everything is still round.
        let mut s = surf64();
        draw64(
            &mut s,
            &Kind::Arc {
                start: 0,
                end: 20,
                value: 1.0,
                thickness: 6,
                fill: Color::WHITE,
                track: Color::TRANSPARENT,
            },
            Rect::new(0, 0, 60, 60),
        );
        assert!(
            lit64(&s, 30, 2),
            "a short sweep from 0 should be at the top"
        );
        assert!(!lit64(&s, 2, 30), "nothing should be at nine o'clock");
    }
}
