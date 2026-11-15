//! Easing functions and small animation helpers. All functions are pure so
//! they can be unit-tested at the boundary values.

#[inline]
pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

#[inline]
pub fn ease_out_back(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let c1 = 1.70158_f32;
    let c3 = c1 + 1.0;
    1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
}

#[inline]
pub fn ease_in_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t.powi(3)
}

/// Linearly interpolate between `a` and `b` by `t`.
#[inline]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ease_endpoints() {
        assert!((ease_out_cubic(0.0)).abs() < 1e-6);
        assert!((ease_out_cubic(1.0) - 1.0).abs() < 1e-6);
        assert!((ease_in_cubic(0.0)).abs() < 1e-6);
        assert!((ease_in_cubic(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn ease_out_cubic_monotone() {
        let mut prev = ease_out_cubic(0.0);
        for i in 1..=20 {
            let v = ease_out_cubic(i as f32 / 20.0);
            assert!(v >= prev - 1e-6);
            prev = v;
        }
    }
}
