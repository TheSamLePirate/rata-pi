//! Animation engine skeleton.
//!
//! Owns an active-animation registry. Callers add animations; the runtime
//! drives them via `tick()` returning `true` while anything is still moving.
//! When nothing is moving, the 60 fps ticker can park.
//!
//! V2.0 lands the scaffolding + easing primitives. V2.1 is the first real
//! consumer (StatusWidget border pulse, retry countdown ring).

#![allow(dead_code)] // exposed; first consumers land in V2.1

pub mod ease;

use std::time::{Duration, Instant};

/// An individual animation.
#[derive(Debug, Clone, Copy)]
pub struct Anim {
    started: Instant,
    duration: Duration,
    /// Optional loop-forever flag for pulses / spinners.
    looping: bool,
}

impl Anim {
    pub fn new(duration: Duration) -> Self {
        Self {
            started: Instant::now(),
            duration,
            looping: false,
        }
    }

    pub fn looping(duration: Duration) -> Self {
        Self {
            started: Instant::now(),
            duration,
            looping: true,
        }
    }

    /// Progress in `[0, 1]`. For looping animations this wraps.
    pub fn progress(&self) -> f64 {
        let elapsed = self.started.elapsed().as_secs_f64();
        let total = self.duration.as_secs_f64().max(0.001);
        let p = elapsed / total;
        if self.looping {
            p.fract()
        } else {
            p.clamp(0.0, 1.0)
        }
    }

    pub fn is_done(&self) -> bool {
        !self.looping && self.started.elapsed() >= self.duration
    }
}

/// Interpolate two RGB components channel-wise.
pub fn lerp_u8(a: u8, b: u8, t: f64) -> u8 {
    let t = t.clamp(0.0, 1.0);
    ((a as f64) * (1.0 - t) + (b as f64) * t)
        .round()
        .clamp(0.0, 255.0) as u8
}

/// Linear interpolation for floats.
pub fn lerp_f64(a: f64, b: f64, t: f64) -> f64 {
    a * (1.0 - t) + b * t
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn anim_progresses() {
        let a = Anim::new(Duration::from_millis(40));
        sleep(Duration::from_millis(20));
        let p = a.progress();
        assert!(p > 0.2 && p < 0.9, "progress in range: got {p}");
        sleep(Duration::from_millis(30));
        assert_eq!(a.progress(), 1.0);
        assert!(a.is_done());
    }

    #[test]
    fn looping_never_done_and_wraps() {
        let a = Anim::looping(Duration::from_millis(10));
        sleep(Duration::from_millis(25));
        let p = a.progress();
        assert!(p < 1.0);
        assert!(!a.is_done());
    }

    #[test]
    fn lerp_u8_endpoints() {
        assert_eq!(lerp_u8(0, 100, 0.0), 0);
        assert_eq!(lerp_u8(0, 100, 1.0), 100);
        assert_eq!(lerp_u8(0, 100, 0.5), 50);
    }
}
