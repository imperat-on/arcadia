//! Pure calculations for the Home hero animation.
use std::time::Duration;

pub(crate) const HERO_ZOOM_DURATION: Duration = Duration::from_millis(900);

/// Translate artwork down as its scroll container moves up, capped by the
/// overscan on each side. Reduced motion leaves the artwork stationary.
pub(super) fn parallax(scroll: f32, height: f32, reduced: bool) -> f32 {
    if reduced || !scroll.is_finite() || !height.is_finite() {
        return 0.0;
    }
    (scroll.max(0.0) * 0.2).min(height.max(0.0) * 0.15)
}

/// Redraw only while interpolation-based Home animations are active.
pub(crate) fn animation_interval(active: bool) -> Option<Duration> {
    active.then_some(Duration::from_millis(33))
}

/// Rotate the featured game without scheduling a render on every frame.
pub(crate) fn carousel_interval() -> Duration {
    Duration::from_secs(6)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallax_is_bounded_and_respects_reduced_motion() {
        assert_eq!(parallax(-20.0, 400.0, false), 0.0);
        assert_eq!(parallax(100.0, 400.0, false), 20.0);
        assert!((parallax(1000.0, 400.0, false) - 60.0).abs() < 0.001);
        assert_eq!(parallax(100.0, 400.0, true), 0.0);
        assert_eq!(parallax(f32::NAN, 400.0, false), 0.0);
    }

    #[test]
    fn carousel_rotation_uses_six_second_interval() {
        assert_eq!(carousel_interval(), Duration::from_secs(6));
    }

    #[test]
    fn animated_home_uses_frame_interval() {
        assert_eq!(animation_interval(true), Some(Duration::from_millis(33)));
    }

    #[test]
    fn idle_home_does_not_schedule_frame_redraws() {
        assert_eq!(animation_interval(false), None);
    }

    #[test]
    fn hero_zoom_finishes_before_the_next_carousel_rotation() {
        assert_eq!(HERO_ZOOM_DURATION, Duration::from_millis(900));
        assert!(HERO_ZOOM_DURATION < carousel_interval());
    }
}
