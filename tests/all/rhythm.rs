use std::time::Duration;

use rs_typing_trainer::practice::rhythm::{beat_index, default_bpm, median_beat_deviation_ms, recommend_rhythm};

#[test]
fn beat_schedule_and_default_bpm_use_injected_time() {
    assert!((default_bpm(200.0) - 270.0).abs() < 1e-9);
    assert_eq!(beat_index(120.0, Duration::from_millis(0)), 0);
    assert_eq!(beat_index(120.0, Duration::from_millis(499)), 0);
    assert_eq!(beat_index(120.0, Duration::from_millis(500)), 1);
}

#[test]
fn median_absolute_deviation_from_nearest_beat_is_scored() {
    let times = [Duration::from_millis(490), Duration::from_millis(1_020), Duration::from_millis(1_600)];
    assert_eq!(median_beat_deviation_ms(120.0, &times), 20.0);
}

#[test]
fn rhythm_is_recommended_only_when_its_axis_is_below_the_lower_quartile() {
    assert!(recommend_rhythm(20.0, &[40.0, 50.0, 60.0, 70.0]));
    assert!(!recommend_rhythm(55.0, &[40.0, 50.0, 60.0, 70.0]));
}
