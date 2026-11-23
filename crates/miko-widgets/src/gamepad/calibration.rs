//! Validated per-control normalization shared by gamepad input paths.
use miko_core::{MikoError, MikoResult};
use schemars::JsonSchema;
use serde::Deserialize;

use super::hid::{Axis, PadEvent};

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct StickCalibration {
    pub deadzone: f32,
    pub sensitivity: f32,
    pub invert_x: bool,
    pub invert_y: bool,
}

impl Default for StickCalibration {
    fn default() -> Self {
        Self {
            deadzone: 0.0,
            sensitivity: 1.0,
            invert_x: false,
            invert_y: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Calibration {
    pub left: StickCalibration,
    pub right: StickCalibration,
    pub trigger_threshold: f32,
}

impl Calibration {
    pub fn validate(self) -> MikoResult<Self> {
        let valid = |s: StickCalibration| {
            s.deadzone.is_finite()
                && (0.0..=0.95).contains(&s.deadzone)
                && s.sensitivity.is_finite()
                && (0.1..=5.0).contains(&s.sensitivity)
        };
        if !valid(self.left)
            || !valid(self.right)
            || !self.trigger_threshold.is_finite()
            || !(0.0..=0.95).contains(&self.trigger_threshold)
        {
            return Err(MikoError::usage("invalid gamepad calibration"));
        }
        Ok(self)
    }

    pub fn apply(self, event: PadEvent) -> PadEvent {
        let PadEvent::Axis(axis, value) = event else {
            return event;
        };
        // A malformed host value must not propagate to the HID report.
        let value = if value.is_finite() { value } else { 0.0 };
        let adjusted = match axis {
            Axis::LeftX => stick(value, self.left, self.left.invert_x),
            Axis::LeftY => stick(value, self.left, self.left.invert_y),
            Axis::RightX => stick(value, self.right, self.right.invert_x),
            Axis::RightY => stick(value, self.right, self.right.invert_y),
            Axis::LeftTrigger | Axis::RightTrigger => {
                ((value.clamp(0.0, 1.0) - self.trigger_threshold) / (1.0 - self.trigger_threshold))
                    .clamp(0.0, 1.0)
            }
        };
        PadEvent::Axis(axis, adjusted)
    }
}

fn stick(value: f32, calibration: StickCalibration, invert: bool) -> f32 {
    let value = value.clamp(-1.0, 1.0);
    let magnitude =
        ((value.abs() - calibration.deadzone) / (1.0 - calibration.deadzone)).clamp(0.0, 1.0);
    let value = value.signum() * (magnitude * calibration.sensitivity).min(1.0);
    if invert { -value } else { value }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_preserve_input_and_calibration_enforces_bounds() {
        let default = Calibration::default().validate().unwrap();
        assert_eq!(
            default.apply(PadEvent::Axis(Axis::LeftX, -0.5)),
            PadEvent::Axis(Axis::LeftX, -0.5)
        );
        assert_eq!(
            default.apply(PadEvent::Axis(Axis::RightTrigger, 0.5)),
            PadEvent::Axis(Axis::RightTrigger, 0.5)
        );
        let mut invalid = default;
        invalid.left.deadzone = 1.0;
        assert!(invalid.validate().is_err());
        invalid.left.deadzone = f32::NAN;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn deadzone_sensitivity_inversion_and_trigger_threshold_apply_independently() {
        let mut c = Calibration::default();
        c.left.deadzone = 0.2;
        c.left.sensitivity = 2.0;
        c.left.invert_x = true;
        c.trigger_threshold = 0.25;
        assert_eq!(
            c.apply(PadEvent::Axis(Axis::LeftX, 0.1)),
            PadEvent::Axis(Axis::LeftX, -0.0)
        );
        assert_eq!(
            c.apply(PadEvent::Axis(Axis::LeftX, 0.6)),
            PadEvent::Axis(Axis::LeftX, -1.0)
        );
        assert_eq!(
            c.apply(PadEvent::Axis(Axis::RightY, 0.5)),
            PadEvent::Axis(Axis::RightY, 0.5)
        );
        assert_eq!(
            c.apply(PadEvent::Axis(Axis::LeftTrigger, 0.25)),
            PadEvent::Axis(Axis::LeftTrigger, 0.0)
        );
        assert_eq!(
            c.apply(PadEvent::Axis(Axis::RightTrigger, 1.0)),
            PadEvent::Axis(Axis::RightTrigger, 1.0)
        );
        assert_eq!(
            c.apply(PadEvent::Axis(Axis::LeftY, f32::NAN)),
            PadEvent::Axis(Axis::LeftY, 0.0)
        );
    }
}
