use rs_typing_trainer::analysis::metrics::{shrink_mean, shrink_rate};

#[test]
fn zero_observations_equal_the_class_prior() {
    assert_eq!(shrink_mean(0, 999.0, 180.0, 10.0), 180.0);
    assert_eq!(shrink_rate(0, 0, 0.08, 10.0), 0.08);
}

#[test]
fn large_samples_approach_the_sample_value() {
    assert!((shrink_mean(10_000, 250.0, 180.0, 10.0) - 250.0).abs() < 0.1);
    assert!((shrink_rate(200, 10, 0.20, 10.0) - 0.05).abs() < 0.01);
}

#[test]
fn changing_k_changes_the_amount_of_shrinkage() {
    let weak = shrink_mean(2, 300.0, 100.0, 1.0);
    let strong = shrink_mean(2, 300.0, 100.0, 20.0);
    assert!(weak > strong);

    let weak_rate = shrink_rate(2, 1, 0.1, 1.0);
    let strong_rate = shrink_rate(2, 1, 0.1, 20.0);
    assert!(weak_rate > strong_rate);
}
