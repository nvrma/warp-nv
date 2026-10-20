use std::time::Duration;

use instant::Instant;
use pathfinder_color::ColorU;

use super::*;

fn blocked() -> ConversationStatus {
    ConversationStatus::Blocked {
        blocked_action: "Run `rm -rf build`?".to_owned(),
    }
}

#[test]
fn activity_from_status_maps_every_status() {
    let cases = [
        (Some(blocked()), false, AgentActivity::NeedsInput),
        (
            Some(ConversationStatus::Error),
            false,
            AgentActivity::Errored,
        ),
        (
            Some(ConversationStatus::InProgress),
            false,
            AgentActivity::Working,
        ),
        (
            Some(ConversationStatus::TransientError),
            false,
            AgentActivity::Working,
        ),
        (
            Some(ConversationStatus::WaitingForEvents),
            false,
            AgentActivity::Working,
        ),
        (Some(ConversationStatus::Success), true, AgentActivity::Done),
        (
            Some(ConversationStatus::Cancelled),
            true,
            AgentActivity::Done,
        ),
        (
            Some(ConversationStatus::Success),
            false,
            AgentActivity::Idle,
        ),
        (
            Some(ConversationStatus::Cancelled),
            false,
            AgentActivity::Idle,
        ),
        (None, true, AgentActivity::Idle),
        (None, false, AgentActivity::Idle),
    ];
    for (status, has_unread, expected) in cases {
        assert_eq!(
            AgentActivity::from_status(status.as_ref(), has_unread),
            expected,
            "status {status:?}, has_unread {has_unread}"
        );
    }
}

#[test]
fn activity_sorts_most_urgent_first() {
    let mut activities = vec![
        AgentActivity::Idle,
        AgentActivity::Working,
        AgentActivity::Done,
        AgentActivity::NeedsInput,
        AgentActivity::Errored,
    ];
    activities.sort();
    assert_eq!(
        activities,
        vec![
            AgentActivity::NeedsInput,
            AgentActivity::Errored,
            AgentActivity::Done,
            AgentActivity::Working,
            AgentActivity::Idle,
        ]
    );
}

#[test]
fn only_waiting_states_need_attention() {
    assert!(AgentActivity::NeedsInput.needs_attention());
    assert!(AgentActivity::Errored.needs_attention());
    assert!(AgentActivity::Done.needs_attention());
    assert!(!AgentActivity::Working.needs_attention());
    assert!(!AgentActivity::Idle.needs_attention());
}

#[test]
fn cycle_phase_wraps_and_applies_offset() {
    let period = Duration::from_millis(1000);
    assert_eq!(cycle_phase(Duration::ZERO, period, 0.), 0.);
    assert!((cycle_phase(Duration::from_millis(250), period, 0.) - 0.25).abs() < 1e-5);
    assert!((cycle_phase(Duration::from_millis(1250), period, 0.) - 0.25).abs() < 1e-5);
    assert!((cycle_phase(Duration::from_millis(750), period, 0.5) - 0.25).abs() < 1e-5);
}

#[test]
fn sonar_ring_expands_thins_and_fades() {
    let start = sonar_ring(0.);
    let end = sonar_ring(1.);
    assert_eq!(start.spread, 0.);
    assert_eq!(end.spread, 1.);
    assert!(start.alpha > end.alpha);
    assert_eq!(end.alpha, 0.);
    assert!(start.width > end.width);

    let mut previous = sonar_ring(0.);
    for step in 1..=10 {
        let ring = sonar_ring(step as f32 / 10.);
        assert!(ring.spread >= previous.spread);
        assert!(ring.alpha <= previous.alpha);
        previous = ring;
    }
}

#[test]
fn breath_swells_and_returns() {
    assert!(breath(0.).abs() < 1e-6);
    assert!((breath(0.5) - 1.).abs() < 1e-6);
    assert!(breath(1.).abs() < 1e-5);
}

#[test]
fn easings_hit_their_endpoints() {
    for ease in [ease_out_cubic, ease_out_back] {
        assert!(ease(0.).abs() < 1e-6);
        assert!((ease(1.) - 1.).abs() < 1e-6);
    }
    assert!(
        (0..100).any(|step| ease_out_back(step as f32 / 100.) > 1.),
        "ease_out_back should overshoot"
    );
}

#[test]
fn with_alpha_scales_existing_alpha() {
    let color = ColorU::new(10, 20, 30, 200);
    assert_eq!(with_alpha(color, 1.), color);
    assert_eq!(with_alpha(color, 0.5), ColorU::new(10, 20, 30, 100));
    assert_eq!(with_alpha(color, 2.).a, 200);
    assert_eq!(with_alpha(color, -1.).a, 0);
}

#[test]
fn waiting_beacon_settles_after_its_active_window() {
    let color = ColorU::new(255, 200, 0, 255);
    let beacon = |activity| AgentBeacon::new(activity, color, 14.);
    let long_ago = Instant::now() - SONAR_ACTIVE_WINDOW - Duration::from_secs(1);

    assert!(beacon(AgentActivity::NeedsInput).is_animating());
    assert!(
        beacon(AgentActivity::NeedsInput)
            .with_waiting_since(Some(Instant::now()))
            .is_animating()
    );
    assert!(
        !beacon(AgentActivity::NeedsInput)
            .with_waiting_since(Some(long_ago))
            .is_animating()
    );
    assert!(
        beacon(AgentActivity::Working)
            .with_waiting_since(Some(long_ago))
            .is_animating()
    );
    assert!(!beacon(AgentActivity::Done).is_animating());
    assert!(!beacon(AgentActivity::Errored).is_animating());
    assert!(!beacon(AgentActivity::Idle).is_animating());
}

#[test]
fn spinner_arc_stays_within_its_sweep_bounds() {
    for step in 0..200 {
        let arc = spinner_arc(Duration::from_millis(step * 37));
        assert!((0. ..std::f32::consts::TAU).contains(&arc.head));
        assert!(arc.sweep >= SPINNER_MIN_SWEEP - 1e-5);
        assert!(arc.sweep <= SPINNER_MAX_SWEEP + 1e-5);
    }
    assert!((spinner_arc(Duration::ZERO).sweep - SPINNER_MIN_SWEEP).abs() < 1e-5);
}
