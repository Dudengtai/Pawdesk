//! Shared easing curves (design §3.5).

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

/// Design token aliases as documentation constants.
pub const EASE_SNAPPY_LABEL: &str = "ease.snappy";
pub const EASE_SMOOTH_LABEL: &str = "ease.smooth";
