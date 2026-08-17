//! Shared easing curves (design §2.5 · Appica-style motion).

/// cubic-bezier(0.34, 1.56, 0.64, 1) approximation for snappy bounce.
pub fn ease_snappy(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    // Overshoot ease-out approximation.
    let c = 1.70158 * 1.525;
    let t1 = t - 1.0;
    1.0 + c * t1 * t1 * t1 + (c + 1.0) * t1 * t1
}

/// cubic-bezier(0.22, 0.61, 0.36, 1) smooth decelerate.
pub fn ease_smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    // Smoothstep-like ease-out.
    1.0 - (1.0 - t).powi(3)
}

/// Soft ease-out cubic — gentle start→settle, no overshoot.
pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Silk ease-out quint — longer tail settle for launcher open (no bounce).
pub fn ease_out_quint(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(5)
}

/// Soft ease-in cubic — for close fade/shrink (accelerate into dismiss).
pub fn ease_in_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * t
}

/// Cubic ease-in-out — gather then fly then settle (reminder hop flight).
pub fn ease_in_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        let u = -2.0 * t + 2.0;
        1.0 - (u * u * u) / 2.0
    }
}

/// Appica-like open: light overshoot back ease-out
/// (approx cubic-bezier(0.175, 0.885, 0.32, 1.5)).
/// May briefly exceed 1.0 mid-curve for bounce, ends at 1.0.
pub fn ease_out_back(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let c1 = 1.70158;
    let c3 = c1 + 1.0;
    let t1 = t - 1.0;
    1.0 + c3 * t1 * t1 * t1 + c1 * t1 * t1
}

/// Soft present (~3% overshoot). Dock open: the cat “hands” the card over.
pub fn ease_out_back_soft(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let c1 = 1.12;
    let c3 = c1 + 1.0;
    let t1 = t - 1.0;
    1.0 + c3 * t1 * t1 * t1 + c1 * t1 * t1
}

/// Linear interpolation.
#[inline]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

/// Exponential approach toward target (frame-rate friendly).
/// `speed` ≈ 10–14 → settles in ~80–120ms.
#[inline]
pub fn approach(cur: f32, target: f32, speed: f32, dt: f32) -> f32 {
    if (cur - target).abs() < 0.001 {
        return target;
    }
    let k = (speed * dt).clamp(0.0, 1.0);
    cur + (target - cur) * k
}

/// Staggered 0..1 progress for item `index` given global open progress `global_t`.
///
/// Each item starts after `index * delay_per` and ramps over `span` of global time.
pub fn stagger_t(global_t: f32, index: usize, delay_per: f32, span: f32) -> f32 {
    let start = index as f32 * delay_per;
    let g = global_t.clamp(0.0, 1.0);
    if g <= start {
        return 0.0;
    }
    let local = ((g - start) / span.max(0.001)).clamp(0.0, 1.0);
    ease_smooth(local)
}

/// Design token aliases as documentation constants.
pub const EASE_SNAPPY_LABEL: &str = "ease.snappy";
pub const EASE_SMOOTH_LABEL: &str = "ease.smooth";
pub const EASE_OUT_BACK_LABEL: &str = "ease.out_back";
pub const EASE_OUT_BACK_SOFT_LABEL: &str = "ease.out_back_soft";
pub const EASE_OUT_QUINT_LABEL: &str = "ease.out_quint";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_soft_settles_and_overshoots() {
        assert!((ease_out_back_soft(0.0) - 0.0).abs() < 1e-5);
        assert!((ease_out_back_soft(1.0) - 1.0).abs() < 1e-5);
        let mid = (0..20)
            .map(|i| ease_out_back_soft(i as f32 / 19.0))
            .fold(0.0f32, f32::max);
        assert!(mid > 1.0, "soft back must overshoot, got {mid}");
        assert!(mid < 1.08, "soft back overshoot should stay tiny, got {mid}");
    }
}
