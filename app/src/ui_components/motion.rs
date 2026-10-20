//! Time-driven wrapper elements for entrance motion and countdowns. Each element computes its
//! frame from a caller-owned start time, so the animation survives the element being rebuilt on
//! every render, and requests repaints only while it is actually moving.

use std::time::Duration;

use instant::Instant;
use pathfinder_color::ColorU;
use warpui::elements::{CornerRadius, Fill, Point, Radius, SelectableElement};
use warpui::event::DispatchedEvent;
use warpui::geometry::rect::RectF;
use warpui::geometry::vector::{Vector2F, vec2f};
use warpui::{
    AfterLayoutContext, AppContext, Element, EventContext, LayoutContext, PaintContext,
    SizeConstraint,
};

use crate::ui_components::agent_beacon::{ease_out_back, with_alpha};

/// ~60fps for short entrance motion, where smoothness matters more than for ambient animation.
const ENTRANCE_FRAME_INTERVAL: Duration = Duration::from_millis(16);
const COUNTDOWN_FRAME_INTERVAL: Duration = Duration::from_millis(33);

/// Paints its child sliding in horizontally from `distance` pixels away and settling into place
/// with a slight overshoot. A positive `distance` enters from the right.
pub(crate) struct SlideIn {
    child: Box<dyn Element>,
    started_at: Instant,
    duration: Duration,
    distance: f32,
    origin: Option<Point>,
}

impl SlideIn {
    pub(crate) fn new(
        child: Box<dyn Element>,
        started_at: Instant,
        duration: Duration,
        distance: f32,
    ) -> Self {
        Self {
            child,
            started_at,
            duration,
            distance,
            origin: None,
        }
    }
}

impl Element for SlideIn {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.child.layout(constraint, ctx, app)
    }

    fn after_layout(&mut self, ctx: &mut AfterLayoutContext, app: &AppContext) {
        self.child.after_layout(ctx, app);
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, ctx.scene.z_index()));
        let progress = animation_progress(self.started_at.elapsed(), self.duration);
        let offset = self.distance * (1. - ease_out_back(progress));
        self.child.paint(origin + vec2f(offset, 0.), ctx, app);
        if progress < 1. {
            ctx.repaint_after(ENTRANCE_FRAME_INTERVAL);
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.child.size()
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.child.dispatch_event(event, ctx, app)
    }

    fn as_selectable_element(&self) -> Option<&dyn SelectableElement> {
        self.child.as_selectable_element()
    }
}

/// A thin rounded bar that drains from full to empty over `duration`. It holds full while
/// paused, i.e. while `started_at` is `None`.
pub(crate) struct CountdownBar {
    started_at: Option<Instant>,
    duration: Duration,
    color: ColorU,
    track_color: ColorU,
    height: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl CountdownBar {
    pub(crate) fn new(
        started_at: Option<Instant>,
        duration: Duration,
        color: ColorU,
        track_color: ColorU,
        height: f32,
    ) -> Self {
        Self {
            started_at,
            duration,
            color,
            track_color,
            height,
            size: None,
            origin: None,
        }
    }
}

impl Element for CountdownBar {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _: &mut LayoutContext,
        _: &AppContext,
    ) -> Vector2F {
        let width = if constraint.max.x().is_finite() {
            constraint.max.x()
        } else {
            constraint.min.x()
        };
        let size = vec2f(width, self.height);
        self.size = Some(size);
        size
    }

    fn after_layout(&mut self, _: &mut AfterLayoutContext, _: &AppContext) {}

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, _: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, ctx.scene.z_index()));
        let Some(size) = self.size else {
            return;
        };
        let rounded = CornerRadius::with_all(Radius::Percentage(50.));
        ctx.scene
            .draw_rect_without_hit_recording(RectF::new(origin, size))
            .with_background(self.track_color)
            .with_corner_radius(rounded);

        let remaining = self.started_at.map_or(1., |started_at| {
            1. - animation_progress(started_at.elapsed(), self.duration)
        });
        let fill_width = size.x() * remaining;
        if fill_width > 0. {
            // Fades in from the left so the bar reads as draining toward its bright leading edge.
            ctx.scene
                .draw_rect_without_hit_recording(RectF::new(origin, vec2f(fill_width, size.y())))
                .with_background(Fill::Gradient {
                    start: vec2f(0., 0.),
                    end: vec2f(1., 0.),
                    start_color: with_alpha(self.color, 0.35),
                    end_color: self.color,
                })
                .with_corner_radius(rounded);
        }
        if self.started_at.is_some() && remaining > 0. {
            ctx.repaint_after(COUNTDOWN_FRAME_INTERVAL);
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        _: &DispatchedEvent,
        _: &mut EventContext,
        _: &AppContext,
    ) -> bool {
        false
    }
}

/// Fraction of `duration` that `elapsed` covers, clamped to `[0, 1]`. A zero duration is
/// immediately complete.
fn animation_progress(elapsed: Duration, duration: Duration) -> f32 {
    if duration.is_zero() {
        return 1.;
    }
    (elapsed.as_secs_f32() / duration.as_secs_f32()).clamp(0., 1.)
}

#[cfg(test)]
#[path = "motion_tests.rs"]
mod tests;
