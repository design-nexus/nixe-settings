//! Display units. Temperatures are stored and sent to hardware in Celsius;
//! only what the user sees and types follows their °C / °F preference.

use crate::prefs::{self, TempUnit};

pub fn fahrenheit() -> bool {
    prefs::get().temp_unit == TempUnit::Fahrenheit
}

pub fn symbol() -> &'static str {
    if fahrenheit() { "°F" } else { "°C" }
}

/// Celsius to the unit being shown.
pub fn from_celsius(c: f64) -> f64 {
    if fahrenheit() { celsius_to_fahrenheit(c) } else { c }
}

/// A value in the unit being shown, back to whole-degree Celsius.
pub fn to_celsius(v: f64) -> i64 {
    if fahrenheit() { fahrenheit_to_celsius(v) } else { v.round() as i64 }
}

/// Spin-button and slider step: 2 °F is about 1 °C, so steps don't repeat a Celsius value.
pub fn step() -> f64 {
    if fahrenheit() { 2.0 } else { 1.0 }
}

pub fn celsius_to_fahrenheit(c: f64) -> f64 {
    c * 9.0 / 5.0 + 32.0
}

pub fn fahrenheit_to_celsius(f: f64) -> i64 {
    ((f - 32.0) * 5.0 / 9.0).round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts() {
        assert_eq!(celsius_to_fahrenheit(0.0), 32.0);
        assert_eq!(celsius_to_fahrenheit(100.0), 212.0);
        assert_eq!(celsius_to_fahrenheit(57.0), 134.6);
        assert_eq!(fahrenheit_to_celsius(212.0), 100);
        assert_eq!(fahrenheit_to_celsius(134.6), 57);
    }

    #[test]
    fn whole_celsius_survives_a_round_trip_through_displayed_fahrenheit() {
        // What the fan curve editor shows (rounded to a whole °F) converts back to the same °C.
        for c in 0..=110 {
            let shown = celsius_to_fahrenheit(c as f64).round();
            assert_eq!(fahrenheit_to_celsius(shown), c, "{c}°C shown as {shown}°F");
        }
    }
}
