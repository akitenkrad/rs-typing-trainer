//! Burst-series and endurance comparison calculations.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BurstSeries {
    runs: Vec<f64>,
}

impl BurstSeries {
    pub fn push(&mut self, wpm: f64) -> (usize, f64) {
        if self.runs.len() < 3 {
            self.runs.push(wpm);
        }
        (self.runs.len(), self.best())
    }

    pub fn best(&self) -> f64 {
        self.runs.iter().copied().max_by(f64::total_cmp).unwrap_or(0.0)
    }
    pub fn complete(&self) -> bool {
        self.runs.len() >= 3
    }
    pub fn run_count(&self) -> usize {
        self.runs.len()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EnduranceComparison {
    pub first_iki_ms: f64,
    pub second_iki_ms: f64,
    pub first_error_rate: f64,
    pub second_error_rate: f64,
    pub warn: bool,
}

pub fn compare_endurance(first_iki: &[f64], second_iki: &[f64], first_error_rate: f64, second_error_rate: f64) -> EnduranceComparison {
    let first = median(first_iki);
    let second = median(second_iki);
    let iki_worse = first > 0.0 && second >= first * 1.15;
    let errors_worse = if first_error_rate == 0.0 {
        second_error_rate > 0.0
    } else {
        second_error_rate >= first_error_rate * 1.5
    };
    EnduranceComparison {
        first_iki_ms: first,
        second_iki_ms: second,
        first_error_rate,
        second_error_rate,
        warn: iki_worse || errors_worse,
    }
}

pub fn compare_endurance_log(log: &crate::engine::session::SessionLog) -> EnduranceComparison {
    let midpoint = log.duration_us / 2;
    let keys = log
        .keystrokes
        .iter()
        .filter(|key| key.kind == crate::engine::session::KeystrokeKind::Char)
        .collect::<Vec<_>>();
    let mut first_iki = Vec::new();
    let mut second_iki = Vec::new();
    for pair in keys.windows(2) {
        let value = pair[1].t_us.saturating_sub(pair[0].t_us) as f64 / 1_000.0;
        if pair[1].t_us < midpoint {
            first_iki.push(value);
        } else if pair[0].t_us >= midpoint {
            second_iki.push(value);
        }
    }
    let (first_total, first_errors) = keys
        .iter()
        .filter(|key| key.t_us < midpoint)
        .fold((0_u64, 0_u64), |(total, errors), key| (total + 1, errors + u64::from(!key.correct)));
    let (second_total, second_errors) = keys
        .iter()
        .filter(|key| key.t_us >= midpoint)
        .fold((0_u64, 0_u64), |(total, errors), key| (total + 1, errors + u64::from(!key.correct)));
    compare_endurance(
        &first_iki,
        &second_iki,
        rate(first_errors, first_total),
        rate(second_errors, second_total),
    )
}

fn rate(part: u64, total: u64) -> f64 {
    if total == 0 { 0.0 } else { part as f64 / total as f64 }
}

fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    }
}
