// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Narrowly adapted from DioxusLabs/blitz commit
// d788124ab881f9bb537cb452ec1d837604a374a8:
// - examples/screenshot.rs
// - packages/blitz-paint/src/render.rs
// - crates/anyrender_svg (parsed SVG tree execution)
// The source project is licensed MIT OR Apache-2.0.

//! Shared CSS-pixel geometry conversion to Kurbo; no pixel buffers are allocated.

use moli_layout::{
    PaintCornerRadii, PaintCornerRadius, PaintLineCap, PaintPath, PaintPathElement, PaintRect,
    PaintShape, PaintTransform2D,
};
use peniko::kurbo::{Affine, BezPath, Cap, Rect};

pub(crate) fn to_backend_shape(shape: &PaintShape) -> BezPath {
    match shape {
        PaintShape::Rect(rect) => to_backend_rect(*rect)
            .map(|rect| rect_path(rect, true))
            .unwrap_or_default(),
        PaintShape::RoundedRect { rect, radii } => to_backend_rect(*rect)
            .map(|rect| {
                rounded_rect_path(
                    rect,
                    normalize_radii(*radii, rect.width(), rect.height()),
                    true,
                )
            })
            .unwrap_or_default(),
        PaintShape::Path(path) => to_backend_path(path),
    }
}

pub(crate) fn to_backend_path(path: &PaintPath) -> BezPath {
    let mut backend = BezPath::new();
    for element in &path.elements {
        match *element {
            PaintPathElement::MoveTo(point) => backend.move_to((point.x, point.y)),
            PaintPathElement::LineTo(point) => backend.line_to((point.x, point.y)),
            PaintPathElement::QuadTo(control, point) => {
                backend.quad_to((control.x, control.y), (point.x, point.y));
            }
            PaintPathElement::CubicTo(first, second, point) => {
                backend.curve_to((first.x, first.y), (second.x, second.y), (point.x, point.y))
            }
            PaintPathElement::Close => backend.close_path(),
        }
    }
    backend
}

pub(crate) const fn to_backend_cap(cap: PaintLineCap) -> Cap {
    match cap {
        PaintLineCap::Butt => Cap::Butt,
        PaintLineCap::Round => Cap::Round,
        PaintLineCap::Square => Cap::Square,
    }
}

pub(crate) fn rounded_rect_path(rect: Rect, radii: PaintCornerRadii, clockwise: bool) -> BezPath {
    if radii.is_zero() {
        return rect_path(rect, clockwise);
    }
    const KAPPA: f64 = 0.552_284_749_830_793_6;
    let tl = radius_pair(radii.top_left);
    let tr = radius_pair(radii.top_right);
    let br = radius_pair(radii.bottom_right);
    let bl = radius_pair(radii.bottom_left);
    let mut path = BezPath::new();
    if clockwise {
        path.move_to((rect.x0 + tl.0, rect.y0));
        path.line_to((rect.x1 - tr.0, rect.y0));
        path.curve_to(
            (rect.x1 - tr.0 + tr.0 * KAPPA, rect.y0),
            (rect.x1, rect.y0 + tr.1 - tr.1 * KAPPA),
            (rect.x1, rect.y0 + tr.1),
        );
        path.line_to((rect.x1, rect.y1 - br.1));
        path.curve_to(
            (rect.x1, rect.y1 - br.1 + br.1 * KAPPA),
            (rect.x1 - br.0 + br.0 * KAPPA, rect.y1),
            (rect.x1 - br.0, rect.y1),
        );
        path.line_to((rect.x0 + bl.0, rect.y1));
        path.curve_to(
            (rect.x0 + bl.0 - bl.0 * KAPPA, rect.y1),
            (rect.x0, rect.y1 - bl.1 + bl.1 * KAPPA),
            (rect.x0, rect.y1 - bl.1),
        );
        path.line_to((rect.x0, rect.y0 + tl.1));
        path.curve_to(
            (rect.x0, rect.y0 + tl.1 - tl.1 * KAPPA),
            (rect.x0 + tl.0 - tl.0 * KAPPA, rect.y0),
            (rect.x0 + tl.0, rect.y0),
        );
    } else {
        path.move_to((rect.x0 + tl.0, rect.y0));
        path.curve_to(
            (rect.x0 + tl.0 - tl.0 * KAPPA, rect.y0),
            (rect.x0, rect.y0 + tl.1 - tl.1 * KAPPA),
            (rect.x0, rect.y0 + tl.1),
        );
        path.line_to((rect.x0, rect.y1 - bl.1));
        path.curve_to(
            (rect.x0, rect.y1 - bl.1 + bl.1 * KAPPA),
            (rect.x0 + bl.0 - bl.0 * KAPPA, rect.y1),
            (rect.x0 + bl.0, rect.y1),
        );
        path.line_to((rect.x1 - br.0, rect.y1));
        path.curve_to(
            (rect.x1 - br.0 + br.0 * KAPPA, rect.y1),
            (rect.x1, rect.y1 - br.1 + br.1 * KAPPA),
            (rect.x1, rect.y1 - br.1),
        );
        path.line_to((rect.x1, rect.y0 + tr.1));
        path.curve_to(
            (rect.x1, rect.y0 + tr.1 - tr.1 * KAPPA),
            (rect.x1 - tr.0 + tr.0 * KAPPA, rect.y0),
            (rect.x1 - tr.0, rect.y0),
        );
        path.line_to((rect.x0 + tl.0, rect.y0));
    }
    path.close_path();
    path
}

pub(crate) fn rect_path(rect: Rect, clockwise: bool) -> BezPath {
    let mut path = BezPath::new();
    if clockwise {
        path.move_to((rect.x0, rect.y0));
        path.line_to((rect.x1, rect.y0));
        path.line_to((rect.x1, rect.y1));
        path.line_to((rect.x0, rect.y1));
    } else {
        path.move_to((rect.x0, rect.y0));
        path.line_to((rect.x0, rect.y1));
        path.line_to((rect.x1, rect.y1));
        path.line_to((rect.x1, rect.y0));
    }
    path.close_path();
    path
}

pub(crate) fn normalize_radii(
    radii: PaintCornerRadii,
    rect_width: f64,
    rect_height: f64,
) -> PaintCornerRadii {
    let mut radii = PaintCornerRadii {
        top_left: sanitize_radius(radii.top_left),
        top_right: sanitize_radius(radii.top_right),
        bottom_right: sanitize_radius(radii.bottom_right),
        bottom_left: sanitize_radius(radii.bottom_left),
    };
    // Tailwind's rounded-full can resolve to f32::MAX. Promote before adding
    // corner pairs so their sum cannot overflow and collapse the scale to zero.
    let factors = [
        ratio(
            rect_width,
            f64::from(radii.top_left.x) + f64::from(radii.top_right.x),
        ),
        ratio(
            rect_width,
            f64::from(radii.bottom_left.x) + f64::from(radii.bottom_right.x),
        ),
        ratio(
            rect_height,
            f64::from(radii.top_left.y) + f64::from(radii.bottom_left.y),
        ),
        ratio(
            rect_height,
            f64::from(radii.top_right.y) + f64::from(radii.bottom_right.y),
        ),
    ];
    let factor = factors.into_iter().fold(1.0_f64, f64::min).min(1.0);
    for radius in [
        &mut radii.top_left,
        &mut radii.top_right,
        &mut radii.bottom_right,
        &mut radii.bottom_left,
    ] {
        radius.x = (f64::from(radius.x) * factor) as f32;
        radius.y = (f64::from(radius.y) * factor) as f32;
    }
    radii
}

fn ratio(available: f64, requested: f64) -> f64 {
    if requested > 0.0 {
        (available.max(0.0) / requested).max(0.0)
    } else {
        1.0
    }
}

fn sanitize_radius(radius: PaintCornerRadius) -> PaintCornerRadius {
    PaintCornerRadius::new(finite_nonnegative(radius.x), finite_nonnegative(radius.y))
}

fn radius_pair(radius: PaintCornerRadius) -> (f64, f64) {
    (f64::from(radius.x), f64::from(radius.y))
}

pub(crate) fn outset_radii(radii: PaintCornerRadii, spread: f32) -> PaintCornerRadii {
    let outset = |radius: PaintCornerRadius| {
        PaintCornerRadius::new((radius.x + spread).max(0.0), (radius.y + spread).max(0.0))
    };
    PaintCornerRadii {
        top_left: outset(radii.top_left),
        top_right: outset(radii.top_right),
        bottom_right: outset(radii.bottom_right),
        bottom_left: outset(radii.bottom_left),
    }
}

pub(crate) fn average_radius(radii: PaintCornerRadii) -> f64 {
    let values = [
        radii.top_left,
        radii.top_right,
        radii.bottom_right,
        radii.bottom_left,
    ];
    values
        .into_iter()
        .map(|radius| f64::from(radius.x + radius.y) * 0.5)
        .sum::<f64>()
        / 4.0
}

pub(crate) fn device_transform(transform: PaintTransform2D, scale: f64) -> Affine {
    Affine::scale(scale) * to_backend_transform(transform)
}

pub(crate) fn to_backend_transform(transform: PaintTransform2D) -> Affine {
    if transform.is_finite() {
        Affine::new(transform.coefficients)
    } else {
        Affine::IDENTITY
    }
}

pub(crate) fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

pub(crate) fn finite_nonnegative(value: f32) -> f32 {
    finite_or_zero(value).max(0.0)
}

pub(crate) fn to_backend_rect(rect: PaintRect) -> Option<Rect> {
    if !rect.x.is_finite()
        || !rect.y.is_finite()
        || !rect.width.is_finite()
        || !rect.height.is_finite()
        || rect.width <= 0.0
        || rect.height <= 0.0
    {
        return None;
    }

    Some(Rect::new(
        f64::from(rect.x),
        f64::from(rect.y),
        f64::from(rect.x) + f64::from(rect.width),
        f64::from(rect.y) + f64::from(rect.height),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maximum_corner_radii_preserve_pill_shapes_at_normal_and_subpixel_sizes() {
        let radii = PaintCornerRadii::all(PaintCornerRadius::new(f32::MAX, f32::MAX));
        for (width, height) in [(120.0, 40.0), (3e-6, 1e-6)] {
            let normalized = normalize_radii(radii, width, height);
            for radius in [
                normalized.top_left,
                normalized.top_right,
                normalized.bottom_right,
                normalized.bottom_left,
            ] {
                let expected = height / 2.0;
                assert!((f64::from(radius.x) / expected - 1.0).abs() < 1e-6);
                assert!((f64::from(radius.y) / expected - 1.0).abs() < 1e-6);
            }
        }
    }
}
