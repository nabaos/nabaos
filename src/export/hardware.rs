//! Hardware resource definitions for GPIO, sensors, and actuators.
//!
//! Provides types for declaring hardware pins, sensor units, and transform
//! formulas. A `HardwareManifest` (parsed from YAML) enumerates all resources
//! on a device and can be converted into ability registrations.

use serde::{Deserialize, Serialize};

use crate::core::error::Result;
use crate::runtime::plugin::AbilitySource;

// ---------------------------------------------------------------------------
// Pin mode
// ---------------------------------------------------------------------------

/// How a hardware pin is configured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinMode {
    DigitalRead,
    DigitalWrite,
    AnalogRead,
    AnalogWrite,
    Pwm,
    I2c,
    Spi,
}

// ---------------------------------------------------------------------------
// Sensor unit
// ---------------------------------------------------------------------------

/// Physical unit reported by a sensor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensorUnit {
    Celsius,
    Fahrenheit,
    Percent,
    Lux,
    Pascal,
    Boolean,
    Raw,
}

// ---------------------------------------------------------------------------
// Transform formula
// ---------------------------------------------------------------------------

/// Linear transform with optional clamping applied to raw sensor readings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformFormula {
    pub scale: f64,
    pub offset: f64,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl TransformFormula {
    /// Apply the transform: `raw * scale + offset`, then clamp to [min, max].
    pub fn apply(&self, raw: f64) -> f64 {
        let result = raw * self.scale + self.offset;
        match (self.min, self.max) {
            (Some(min), Some(max)) => result.clamp(min, max),
            (Some(min), None) => result.max(min),
            (None, Some(max)) => result.min(max),
            (None, None) => result,
        }
    }
}

// ---------------------------------------------------------------------------
// Hardware resource
// ---------------------------------------------------------------------------

/// A single hardware resource (pin, sensor, actuator, or bus device).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareResource {
    pub name: String,
    pub description: String,
    pub pin: u16,
    pub mode: PinMode,
    pub unit: Option<SensorUnit>,
    pub transform: Option<TransformFormula>,
    pub poll_interval_ms: Option<u64>,
}

impl HardwareResource {
    /// Derive a canonical ability name from this resource.
    ///
    /// Format: `hw.{name}_{suffix}` where suffix is `read`, `write`, or `bus`
    /// depending on the pin mode.
    pub fn ability_name(&self) -> String {
        let suffix = match self.mode {
            PinMode::DigitalRead | PinMode::AnalogRead => "read",
            PinMode::DigitalWrite | PinMode::AnalogWrite | PinMode::Pwm => "write",
            PinMode::I2c | PinMode::Spi => "bus",
        };
        format!("hw.{}_{}", self.name, suffix)
    }

    /// The ability source for hardware resources.
    pub fn to_ability_source(&self) -> AbilitySource {
        AbilitySource::Hardware
    }
}

// ---------------------------------------------------------------------------
// Hardware manifest
// ---------------------------------------------------------------------------

/// Top-level manifest listing all hardware resources on a device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareManifest {
    pub name: String,
    pub version: String,
    pub resources: Vec<HardwareResource>,
}

/// Parse a `HardwareManifest` from a YAML string.
pub fn load_manifest(yaml: &str) -> Result<HardwareManifest> {
    let manifest: HardwareManifest = serde_yaml::from_str(yaml)?;
    Ok(manifest)
}

/// Register abilities for every resource in a manifest.
///
/// Returns `(ability_name, AbilitySource::Hardware)` pairs.
pub fn register_abilities(manifest: &HardwareManifest) -> Vec<(String, AbilitySource)> {
    manifest
        .resources
        .iter()
        .map(|r| (r.ability_name(), r.to_ability_source()))
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_yaml() -> &'static str {
        r#"
name: rpi4-sensor-board
version: "1.0.0"
resources:
  - name: temp
    description: "DHT22 temperature sensor"
    pin: 4
    mode: digital_read
    unit: celsius
    transform:
      scale: 0.1
      offset: -40.0
      min: -40.0
      max: 80.0
    poll_interval_ms: 2000
  - name: led
    description: "Status LED"
    pin: 17
    mode: digital_write
    unit: null
    transform: null
    poll_interval_ms: null
"#
    }

    #[test]
    fn yaml_parse_roundtrip() {
        let manifest = load_manifest(sample_yaml()).expect("should parse");
        assert_eq!(manifest.name, "rpi4-sensor-board");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.resources.len(), 2);

        let temp = &manifest.resources[0];
        assert_eq!(temp.name, "temp");
        assert_eq!(temp.pin, 4);
        assert_eq!(temp.mode, PinMode::DigitalRead);
        assert_eq!(temp.unit, Some(SensorUnit::Celsius));
        assert!(temp.transform.is_some());
        assert_eq!(temp.poll_interval_ms, Some(2000));

        let led = &manifest.resources[1];
        assert_eq!(led.name, "led");
        assert_eq!(led.pin, 17);
        assert_eq!(led.mode, PinMode::DigitalWrite);
        assert_eq!(led.unit, None);
        assert!(led.transform.is_none());
        assert_eq!(led.poll_interval_ms, None);
    }

    #[test]
    fn ability_name_read_pin() {
        let r = HardwareResource {
            name: "temp".into(),
            description: "temperature".into(),
            pin: 4,
            mode: PinMode::DigitalRead,
            unit: Some(SensorUnit::Celsius),
            transform: None,
            poll_interval_ms: None,
        };
        assert_eq!(r.ability_name(), "hw.temp_read");
    }

    #[test]
    fn ability_name_write_pin() {
        let r = HardwareResource {
            name: "led".into(),
            description: "LED".into(),
            pin: 17,
            mode: PinMode::DigitalWrite,
            unit: None,
            transform: None,
            poll_interval_ms: None,
        };
        assert_eq!(r.ability_name(), "hw.led_write");
    }

    #[test]
    fn transform_apply_with_clamp() {
        let t = TransformFormula {
            scale: 100.0,
            offset: 0.0,
            min: Some(0.0),
            max: Some(100.0),
        };
        // 1.5 * 100 + 0 = 150 → clamped to 100
        assert!((t.apply(1.5) - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn empty_manifest() {
        let yaml = r#"
name: empty-board
version: "0.1.0"
resources: []
"#;
        let manifest = load_manifest(yaml).expect("should parse empty manifest");
        assert_eq!(manifest.resources.len(), 0);
    }

    #[test]
    fn manifest_mixed_pin_modes() {
        let yaml = r#"
name: mixed-board
version: "1.0.0"
resources:
  - name: adc
    description: "Analog input"
    pin: 0
    mode: analog_read
    unit: raw
    transform: null
    poll_interval_ms: 100
  - name: pwm_fan
    description: "PWM fan control"
    pin: 12
    mode: pwm
    unit: null
    transform: null
    poll_interval_ms: null
  - name: i2c_sensor
    description: "I2C barometer"
    pin: 2
    mode: i2c
    unit: pascal
    transform: null
    poll_interval_ms: 5000
"#;
        let manifest = load_manifest(yaml).expect("should parse");
        assert_eq!(manifest.resources[0].ability_name(), "hw.adc_read");
        assert_eq!(manifest.resources[1].ability_name(), "hw.pwm_fan_write");
        assert_eq!(manifest.resources[2].ability_name(), "hw.i2c_sensor_bus");
    }

    #[test]
    fn sensor_unit_serde_roundtrip() {
        let units = vec![
            SensorUnit::Celsius,
            SensorUnit::Fahrenheit,
            SensorUnit::Percent,
            SensorUnit::Lux,
            SensorUnit::Pascal,
            SensorUnit::Boolean,
            SensorUnit::Raw,
        ];
        for unit in &units {
            let json = serde_json::to_string(unit).expect("serialize");
            let back: SensorUnit = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(*unit, back);
        }
    }

    #[test]
    fn pin_mode_serde_snake_case() {
        let modes = vec![
            (PinMode::DigitalRead, "\"digital_read\""),
            (PinMode::DigitalWrite, "\"digital_write\""),
            (PinMode::AnalogRead, "\"analog_read\""),
            (PinMode::AnalogWrite, "\"analog_write\""),
            (PinMode::Pwm, "\"pwm\""),
            (PinMode::I2c, "\"i2c\""),
            (PinMode::Spi, "\"spi\""),
        ];
        for (mode, expected_json) in &modes {
            let json = serde_json::to_string(mode).expect("serialize");
            assert_eq!(&json, expected_json, "serialized {:?}", mode);
            let back: PinMode = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(*mode, back);
        }
    }
}
