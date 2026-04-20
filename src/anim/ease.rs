//! Easing functions in `[0,1] -> [0,1]`.
//!
//! Keeping the kit small and recognizable — we use these for color/opacity
//! tweens in the status widget and toast stack.

#![allow(dead_code)]

/// y = x.
pub fn linear(t: f64) -> f64 {
    t.clamp(0.0, 1.0)
}

/// Decelerating cubic; most natural "arriving" feel.
pub fn ease_out_cubic(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    let f = 1.0 - t;
    1.0 - f * f * f
}

/// Smoothstep — C¹ continuous, good for looping pulses.
pub fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Triangle wave — 0 at t=0 and t=1, 1 at t=0.5. Useful for breathing pulses.
pub fn triangle(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (2.0 * t - 1.0).abs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints() {
        assert_eq!(linear(0.0), 0.0);
        assert_eq!(linear(1.0), 1.0);
        assert!((ease_out_cubic(1.0) - 1.0).abs() < 1e-9);
        assert_eq!(smoothstep(0.0), 0.0);
        assert_eq!(smoothstep(1.0), 1.0);
        assert_eq!(triangle(0.0), 0.0);
        assert_eq!(triangle(1.0), 0.0);
        assert_eq!(triangle(0.5), 1.0);
    }

    #[test]
    fn ease_out_is_front_loaded() {
        // 25% through an ease-out has already covered more than a quarter.
        assert!(ease_out_cubic(0.25) > 0.25);
    }
}
