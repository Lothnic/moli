// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Narrowly adapted from DioxusLabs/blitz commit
// d788124ab881f9bb537cb452ec1d837604a374a8:
// - examples/screenshot.rs
// - packages/blitz-paint/src/render.rs
// - crates/anyrender_svg (parsed SVG tree execution)
// The source project is licensed MIT OR Apache-2.0.

//! AnyRender box-shadow and supported filter commands.

use super::to_backend_color;
use crate::geometry::{
    average_radius, device_transform, finite_nonnegative, finite_or_zero, normalize_radii,
    outset_radii, rect_path, rounded_rect_path, to_backend_rect,
};
use anyrender::PaintScene;
use anyrender::{Filter, filters::FilterEffect};
use moli_layout::{PaintBoxShadow, PaintFilter};
use peniko::kurbo::{Affine, Vec2};
use peniko::{Color, Compose, Fill, Mix};

pub(super) fn paint_box_shadow(scene: &mut impl PaintScene, shadow: &PaintBoxShadow, scale: f64) {
    let Some(rect) = to_backend_rect(shadow.rect) else {
        return;
    };
    if shadow.color.alpha <= 0.0 {
        return;
    }
    let spread = finite_nonnegative(shadow.spread_radius) as f64;
    let blur = finite_nonnegative(shadow.blur_radius) as f64;
    let offset = Vec2::new(
        f64::from(finite_or_zero(shadow.offset.x)),
        f64::from(finite_or_zero(shadow.offset.y)),
    );
    let shadow_rect = rect.inflate(spread, spread);
    let radii = normalize_radii(
        outset_radii(shadow.radii, spread as f32),
        shadow_rect.width(),
        shadow_rect.height(),
    );
    let radius = average_radius(radii);
    let transform = device_transform(shadow.transform, scale) * Affine::translate(offset);
    let color = to_backend_color(shadow.color);

    if shadow.inset {
        let clip = rounded_rect_path(
            rect,
            normalize_radii(shadow.radii, rect.width(), rect.height()),
            true,
        );
        scene.push_layer(
            Mix::Normal,
            1.0,
            device_transform(shadow.transform, scale),
            &clip,
            None,
            None,
        );
        scene.fill(
            Fill::NonZero,
            device_transform(shadow.transform, scale),
            color,
            None,
            &clip,
        );
        scene.push_layer(
            Compose::DestOut,
            1.0,
            device_transform(shadow.transform, scale),
            &clip,
            None,
            None,
        );
        scene.draw_box_shadow(transform, shadow_rect, Color::WHITE, radius, blur);
        scene.pop_layer();
        scene.pop_layer();
    } else {
        let extent = spread + blur * 4.0 + offset.hypot() + 1.0;
        let outer = rect.inflate(extent, extent);
        let mut clip = rect_path(outer, true);
        clip.extend(rounded_rect_path(
            rect,
            normalize_radii(shadow.radii, rect.width(), rect.height()),
            false,
        ));
        scene.push_clip_layer(device_transform(shadow.transform, scale), &clip);
        scene.draw_box_shadow(transform, shadow_rect, color, radius, blur);
        scene.pop_layer();
    }
}

pub(crate) fn backend_filter(filter: &PaintFilter) -> Option<Filter> {
    let effect = match *filter {
        PaintFilter::Blur(radius) => FilterEffect::blur(finite_nonnegative(radius)),
        PaintFilter::DropShadow {
            offset,
            blur_radius,
            color,
        } => FilterEffect::drop_shadow(
            finite_or_zero(offset.x),
            finite_or_zero(offset.y),
            finite_nonnegative(blur_radius),
            super::to_backend_color(color),
        ),
        PaintFilter::Brightness(_)
        | PaintFilter::Contrast(_)
        | PaintFilter::Grayscale(_)
        | PaintFilter::HueRotate(_)
        | PaintFilter::Invert(_)
        | PaintFilter::Opacity(_)
        | PaintFilter::Saturate(_)
        | PaintFilter::Sepia(_) => return None,
    };
    Some(Filter::single(effect))
}
