//! Recommendation predicates based on a user's own score distribution.

pub fn recommend_preview(reaction_axis: f64, own_distribution: &[f64]) -> bool {
    reaction_axis < lower_quartile(own_distribution)
}

pub fn lower_quartile(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let index = ((sorted.len() - 1) as f64 * 0.25).floor() as usize;
    sorted[index]
}
