//! A small animated status glyph for agent rows. Agents that need the user "sonar" outward,
//! working agents spin a tapering arc around a breathing core, and settled states hold still so
//! motion is reserved for what is live.

use std::f32::consts::TAU;
use std::sync::OnceLock;
use std::time::Duration;

use instant::Instant;
use pathfinder_color::ColorU;
use warp_core::ui::theme::WarpTheme;
use warpui::elements::{Border, CornerRadius, Point, Radius};
use warpui::event::DispatchedEvent;
use warpui::geometry::rect::RectF;
use warpui::geometry::vector::{Vector2F, vec2f};
use warpui::{
    AfterLayoutContext, AppContext, Element, EventContext, LayoutContext, PaintContext,
    SizeConstraint,
};

use crate::ai::agent::conversation::ConversationStatus;

/// ~30fps, matching the shimmering-text cadence so animated chrome shares repaint frames.
const FRAME_INTERVAL: Duration = Duration::from_millis(33);
const SONAR_PERIOD: Duration = Duration::from_millis(1500);
const BREATH_PERIOD: Duration = Duration::from_millis(1600);
/// One revolution of the working spinner's leading edge.
const SPINNER_PERIOD: Duration = Duration::from_millis(1100);
/// One grow-and-shrink cycle of the spinner's arc, offset from the rotation so the tail appears
/// to chase the head.
const SPINNER_SWEEP_PERIOD: Duration = Duration::from_millis(2300);
const SPINNER_MIN_SWEEP: f32 = 0.25 * std::f32::consts::PI;
const SPINNER_MAX_SWEEP: f32 = 1.4 * std::f32::consts::PI;
/// Dots laid along the arc; dense enough that they overlap into a continuous stroke.
const SPINNER_DOTS: usize = 18;
const SPINNER_STROKE_RADIUS: f32 = 0.95;
/// How long an agent waiting on the user keeps pinging before its beacon settles, so an agent left
/// blocked for hours doesn't keep the window repainting.
const SONAR_ACTIVE_WINDOW: Duration = Duration::from_secs(120);

/// Coarse activity state of an agent, ordered from most to least urgent so a sort puts the agents
/// that need the user first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum AgentActivity {
    /// Blocked on a permission request, question, or approval.
    NeedsInput,
    Errored,
    /// Finished a turn the user hasn't looked at yet.
    Done,
    Working,
    /// Running without rich status, or finished and already seen.
    Idle,
}

impl AgentActivity {
    /// `status` is the agent's rich status when known; `has_unread` is whether the agent has an
    /// unread notification, which keeps a finished agent flagged until the user views it.
    pub(crate) fn from_status(status: Option<&ConversationStatus>, has_unread: bool) -> Self {
        match status {
            Some(ConversationStatus::Blocked { .. }) => Self::NeedsInput,
            Some(ConversationStatus::Error) => Self::Errored,
            Some(
                ConversationStatus::InProgress
                | ConversationStatus::TransientError
                | ConversationStatus::WaitingForEvents,
            ) => Self::Working,
            Some(ConversationStatus::Success | ConversationStatus::Cancelled) if has_unread => {
                Self::Done
            }
            Some(ConversationStatus::Success | ConversationStatus::Cancelled) | None => Self::Idle,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::NeedsInput => "Needs input",
            Self::Errored => "Error",
            Self::Done => "Done",
            Self::Working => "Working",
            Self::Idle => "Idle",
        }
    }

    /// Whether the agent is waiting on the user in some way.
    pub(crate) fn needs_attention(self) -> bool {
        match self {
            Self::NeedsInput | Self::Errored | Self::Done => true,
            Self::Working | Self::Idle => false,
        }
    }

    /// Mirrors the palette of [`ConversationStatus::status_icon_and_color`] so the beacon agrees
    /// with status badges elsewhere in the app.
    pub(crate) fn color(self, theme: &WarpTheme) -> ColorU {
        match self {
            Self::NeedsInput => theme.ansi_fg_yellow(),
            Self::Errored => theme.ansi_fg_red(),
            Self::Done => theme.ansi_fg_green(),
            Self::Working => theme.ansi_fg_magenta(),
            Self::Idle => theme.sub_text_color(theme.background()).into_solid(),
        }
    }
}

pub(crate) struct AgentBeacon {
    activity: AgentActivity,
    color: ColorU,
    diameter: f32,
    waiting_since: Option<Instant>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl AgentBeacon {
    pub(crate) fn new(activity: AgentActivity, color: ColorU, diameter: f32) -> Self {
        Self {
            activity,
            color,
            diameter,
            waiting_since: None,
            size: None,
            origin: None,
        }
    }

    /// When the agent began waiting on the user, which bounds how long the beacon pings.
    pub(crate) fn with_waiting_since(mut self, waiting_since: Option<Instant>) -> Self {
        self.waiting_since = waiting_since;
        self
    }

    fn is_animating(&self) -> bool {
        match self.activity {
            AgentActivity::Working => true,
            AgentActivity::NeedsInput => self
                .waiting_since
                .is_none_or(|since| since.elapsed() < SONAR_ACTIVE_WINDOW),
            AgentActivity::Errored | AgentActivity::Done | AgentActivity::Idle => false,
        }
    }
}

impl Element for AgentBeacon {
    fn layout(&mut self, _: SizeConstraint, _: &mut LayoutContext, _: &AppContext) -> Vector2F {
        let size = vec2f(self.diameter, self.diameter);
        self.size = Some(size);
        size
    }

    fn after_layout(&mut self, _: &mut AfterLayoutContext, _: &AppContext) {}

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, _: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, ctx.scene.z_index()));
        let center = origin + vec2f(self.diameter, self.diameter) / 2.;
        let outer_radius = self.diameter / 2.;
        let core_radius = self.diameter * 0.2;
        let elapsed = animation_epoch().elapsed();
        let is_animating = self.is_animating();

        match self.activity {
            AgentActivity::NeedsInput if is_animating => {
                for phase_offset in [0., 0.5] {
                    let ring = sonar_ring(cycle_phase(elapsed, SONAR_PERIOD, phase_offset));
                    let radius = core_radius + (outer_radius - core_radius) * ring.spread;
                    draw_ring(
                        ctx,
                        center,
                        radius,
                        ring.width,
                        with_alpha(self.color, ring.alpha),
                    );
                }
                draw_disc(ctx, center, core_radius, self.color);
            }
            AgentActivity::Working => {
                let breath = breath(cycle_phase(elapsed, BREATH_PERIOD, 0.));
                let core_radius = core_radius * 0.85;
                draw_disc(
                    ctx,
                    center,
                    core_radius + 0.5 + breath,
                    with_alpha(self.color, 0.25 * breath),
                );
                draw_disc(
                    ctx,
                    center,
                    core_radius,
                    with_alpha(self.color, 0.75 + 0.25 * breath),
                );

                let arc_radius = outer_radius - 1.2;
                // Borders are drawn inside their bounds, so grow the track by half its width to
                // center it under the arc.
                draw_ring(
                    ctx,
                    center,
                    arc_radius + 0.5,
                    1.,
                    with_alpha(self.color, 0.12),
                );
                let arc = spinner_arc(elapsed);
                for index in 0..SPINNER_DOTS {
                    let distance_from_head = index as f32 / (SPINNER_DOTS - 1) as f32;
                    let angle = arc.head - arc.sweep * distance_from_head;
                    let position = center + vec2f(angle.cos(), angle.sin()) * arc_radius;
                    let alpha = (1. - distance_from_head).powf(1.4);
                    draw_disc(
                        ctx,
                        position,
                        SPINNER_STROKE_RADIUS,
                        with_alpha(self.color, alpha),
                    );
                }
            }
            AgentActivity::NeedsInput | AgentActivity::Errored | AgentActivity::Done => {
                draw_ring(
                    ctx,
                    center,
                    outer_radius - 1.5,
                    1.,
                    with_alpha(self.color, 0.35),
                );
                draw_disc(ctx, center, core_radius, self.color);
            }
            AgentActivity::Idle => {
                draw_ring(ctx, center, core_radius + 0.5, 1.5, self.color);
            }
        }

        if is_animating {
            ctx.repaint_after(FRAME_INTERVAL);
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

/// A shared start time so every beacon on screen pulses in phase instead of restarting whenever
/// its row is rebuilt.
fn animation_epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// Position within a repeating cycle of `period`, in `[0, 1)`, shifted by `offset` cycles.
fn cycle_phase(elapsed: Duration, period: Duration, offset: f32) -> f32 {
    let phase = elapsed.as_secs_f32() / period.as_secs_f32() + offset;
    phase.rem_euclid(1.)
}

#[derive(Debug, PartialEq)]
struct SonarRing {
    /// Fraction of the distance from the core to the beacon's edge.
    spread: f32,
    alpha: f32,
    width: f32,
}

/// A ring that eases outward from the core while thinning and fading, so the motion reads as a
/// ping rather than a uniform expansion.
fn sonar_ring(phase: f32) -> SonarRing {
    let phase = phase.clamp(0., 1.);
    SonarRing {
        spread: ease_out_cubic(phase),
        alpha: (1. - phase).powf(1.6) * 0.85,
        width: 1.5 - 0.75 * phase,
    }
}

#[derive(Debug)]
struct SpinnerArc {
    /// Angle of the arc's leading edge, in radians.
    head: f32,
    /// Angular length of the arc trailing behind the head, in radians.
    sweep: f32,
}

fn spinner_arc(elapsed: Duration) -> SpinnerArc {
    let swell = breath(cycle_phase(elapsed, SPINNER_SWEEP_PERIOD, 0.));
    SpinnerArc {
        head: TAU * cycle_phase(elapsed, SPINNER_PERIOD, 0.),
        sweep: SPINNER_MIN_SWEEP + (SPINNER_MAX_SWEEP - SPINNER_MIN_SWEEP) * swell,
    }
}

/// A smooth 0 → 1 → 0 swell over one cycle.
fn breath(phase: f32) -> f32 {
    0.5 - 0.5 * (TAU * phase).cos()
}

pub(crate) fn ease_out_cubic(t: f32) -> f32 {
    1. - (1. - t.clamp(0., 1.)).powi(3)
}

/// Ease-out with a slight overshoot past the target before settling.
pub(crate) fn ease_out_back(t: f32) -> f32 {
    const OVERSHOOT: f32 = 1.4;
    let t = t.clamp(0., 1.) - 1.;
    1. + (OVERSHOOT + 1.) * t.powi(3) + OVERSHOOT * t.powi(2)
}

pub(crate) fn with_alpha(color: ColorU, alpha: f32) -> ColorU {
    let alpha = (color.a as f32 * alpha.clamp(0., 1.)).round() as u8;
    ColorU::new(color.r, color.g, color.b, alpha)
}

fn circle_bounds(center: Vector2F, radius: f32) -> RectF {
    RectF::new(center - vec2f(radius, radius), vec2f(radius, radius) * 2.)
}

fn draw_disc(ctx: &mut PaintContext, center: Vector2F, radius: f32, color: ColorU) {
    if radius <= 0. || color.a == 0 {
        return;
    }
    ctx.scene
        .draw_rect_without_hit_recording(circle_bounds(center, radius))
        .with_background(color)
        .with_corner_radius(CornerRadius::with_all(Radius::Percentage(50.)));
}

fn draw_ring(ctx: &mut PaintContext, center: Vector2F, radius: f32, width: f32, color: ColorU) {
    if radius <= 0. || color.a == 0 {
        return;
    }
    ctx.scene
        .draw_rect_without_hit_recording(circle_bounds(center, radius))
        .with_border(Border::all(width).with_border_color(color))
        .with_corner_radius(CornerRadius::with_all(Radius::Percentage(50.)));
}

#[cfg(test)]
#[path = "agent_beacon_tests.rs"]
mod tests;
