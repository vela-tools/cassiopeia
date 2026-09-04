//! Sine-wave pulse animation for indeterminate progress bars.

use num_traits::ToPrimitive;
use smart_default::SmartDefault;
use std::f64::consts::PI;

/// Sine-wave pulse animation for indeterminate progress bars.
#[derive(Debug, SmartDefault)]
pub struct PulseAnimation {
    /// Width, in cells, the pulse sweeps across.
    #[default = 30]
    width: u64,
    /// Number of frames in one full sweep cycle.
    #[default = 60]
    cycle_frames: u64,
}

impl PulseAnimation {
    /// Builds a pulse animation sweeping `width` cells over `cycle_frames` frames.
    #[must_use]
    pub const fn new(width: u64, cycle_frames: u64) -> PulseAnimation {
        PulseAnimation { width, cycle_frames }
    }

    /// Returns the pulse position, in cells, for the given `frame`.
    #[must_use]
    pub fn get_position(&self, frame: u64) -> u64 {
        let cycle = self.cycle_frames.max(1);
        let progress = to_f64(frame % cycle) / to_f64(cycle);
        let sine_value = (progress * PI * 2.0).sin();
        // Map the sine output from [-1, 1] to [0, width].
        let normalized = f64::midpoint(sine_value, 1.0);
        (normalized * to_f64(self.width)).to_u64().unwrap_or(0)
    }
}

/// Widens an animation count to a float. `ToPrimitive` keeps the conversion off the `as` cast lints;
/// animation counts never approach the f64 mantissa limit anyway.
fn to_f64(value: u64) -> f64 {
    value.to_f64().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use crate::animation::pulse::PulseAnimation;

    #[test]
    fn the_pulse_starts_at_the_midpoint() {
        let pulse = PulseAnimation::default();
        // sin(0) maps to the middle of the width.
        assert_eq!(pulse.get_position(0), 15);
    }

    #[test]
    fn the_pulse_peaks_a_quarter_of_the_way_through_the_cycle() {
        let pulse = PulseAnimation::new(30, 60);
        // sin(pi/2) = 1 maps to the full width.
        assert_eq!(pulse.get_position(15), 30);
    }

    #[test]
    fn the_pulse_stays_within_its_width() {
        let pulse = PulseAnimation::default();
        for frame in 0..120 {
            assert!(pulse.get_position(frame) <= 30);
        }
    }
}
