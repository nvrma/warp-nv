use super::badge_label;

#[test]
fn badge_label_hides_zero_and_caps_large_counts() {
    assert_eq!(badge_label(0), None);
    assert_eq!(badge_label(1), Some("1".to_owned()));
    assert_eq!(badge_label(99), Some("99".to_owned()));
    assert_eq!(badge_label(100), Some("99+".to_owned()));
}
