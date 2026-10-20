use std::time::Duration;

use super::animation_progress;

#[test]
fn animation_progress_clamps_to_unit_range() {
    let duration = Duration::from_millis(400);
    assert_eq!(animation_progress(Duration::ZERO, duration), 0.);
    assert!((animation_progress(Duration::from_millis(100), duration) - 0.25).abs() < 1e-6);
    assert_eq!(animation_progress(Duration::from_millis(400), duration), 1.);
    assert_eq!(animation_progress(Duration::from_secs(10), duration), 1.);
}

#[test]
fn zero_duration_is_immediately_complete() {
    assert_eq!(animation_progress(Duration::ZERO, Duration::ZERO), 1.);
}
