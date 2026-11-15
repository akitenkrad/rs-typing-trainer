//! Accuracy-gate eligibility shared by persistence and result presentation.

pub fn is_eligible(min_accuracy: f64, accuracy: f64) -> bool {
    min_accuracy <= 0.0 || accuracy >= min_accuracy
}
