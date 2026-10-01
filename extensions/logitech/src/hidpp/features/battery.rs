//! Battery level: UnifiedBattery (0x1004), BatteryStatus (0x1000) or
//! BatteryVoltage (0x1001), whichever the device has.

use crate::hidpp::device::{Info, be16, feature as f, need};
use crate::hidpp::link::{Link, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
pub enum Charging {
    Discharging,
    Charging,
    Full,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Battery {
    pub percent: u8,
    /// The device only reports a coarse level (critical/low/good/full).
    pub approximate: bool,
    pub charging: Charging,
}

impl Battery {
    pub fn is_low(&self) -> bool {
        self.charging == Charging::Discharging && self.percent <= 15
    }

    pub fn describe(&self) -> String {
        let level = if self.approximate { format!("about {}%", self.percent) } else { format!("{}%", self.percent) };
        match self.charging {
            Charging::Discharging => level,
            Charging::Charging => format!("{level}, charging"),
            Charging::Full => "Full".into(),
            Charging::Error => format!("{level}, charging problem"),
        }
    }
}

fn unified(r: &[u8]) -> Result<Battery> {
    let r = need(r, 3)?;
    let (percent, approximate) = if r[0] > 0 {
        (r[0].min(100), false)
    } else {
        let flags = r[1];
        let p = if flags & 0x08 != 0 {
            90
        } else if flags & 0x04 != 0 {
            50
        } else if flags & 0x02 != 0 {
            20
        } else {
            5
        };
        (p, true)
    };
    let charging = match r[2] {
        0 => Charging::Discharging,
        1 | 2 => Charging::Charging,
        3 => Charging::Full,
        _ => Charging::Error,
    };
    Ok(Battery { percent, approximate, charging })
}

fn status(r: &[u8]) -> Result<Battery> {
    let r = need(r, 3)?;
    let charging = match r[2] {
        0 => Charging::Discharging,
        1 | 2 | 4 => Charging::Charging,
        3 => Charging::Full,
        _ => Charging::Error,
    };
    Ok(Battery { percent: r[0].min(100), approximate: false, charging })
}

/// A rough charge estimate for a single Li-ion cell.
pub fn percent_from_millivolts(mv: u16) -> u8 {
    const CURVE: &[(u16, u8)] =
        &[(4186, 100), (4067, 90), (3989, 80), (3922, 70), (3859, 60), (3811, 50), (3778, 40), (3751, 30), (3717, 20), (3671, 10), (3646, 5), (3579, 2), (3500, 0)];
    if mv >= CURVE[0].0 {
        return 100;
    }
    for w in CURVE.windows(2) {
        let ((hv, hp), (lv, lp)) = (w[0], w[1]);
        if mv >= lv {
            let t = (mv - lv) as f32 / (hv - lv) as f32;
            return (lp as f32 + t * (hp - lp) as f32).round() as u8;
        }
    }
    0
}

fn voltage(r: &[u8]) -> Result<Battery> {
    let r = need(r, 3)?;
    let mv = be16(r, 0);
    let flags = r[2];
    let charging = if flags & 0x80 != 0 {
        match flags & 0x07 {
            0 => Charging::Charging,
            1 => Charging::Full,
            _ => Charging::Error,
        }
    } else {
        Charging::Discharging
    };
    Ok(Battery { percent: percent_from_millivolts(mv), approximate: true, charging })
}

pub fn read(link: &mut Link, info: &Info) -> Result<Option<Battery>> {
    if info.has(f::UNIFIED_BATTERY) {
        return unified(&info.call(link, f::UNIFIED_BATTERY, 1, &[])?).map(Some);
    }
    if info.has(f::BATTERY) {
        return status(&info.call(link, f::BATTERY, 0, &[])?).map(Some);
    }
    if info.has(f::BATTERY_VOLTAGE) {
        return voltage(&info.call(link, f::BATTERY_VOLTAGE, 0, &[])?).map(Some);
    }
    Ok(None)
}

/// A battery change the device announced on its own.
pub fn from_notification(info: &Info, report: &[u8]) -> Option<Battery> {
    if report.len() < 7 || report[1] != info.index || report[3] & 0x0F != 0 {
        return None;
    }
    let params = &report[4..];
    let index = report[2];
    if info.index_of(f::UNIFIED_BATTERY) == Some(index) {
        unified(params).ok()
    } else if info.index_of(f::BATTERY) == Some(index) {
        status(params).ok()
    } else if info.index_of(f::BATTERY_VOLTAGE) == Some(index) {
        voltage(params).ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unified_levels() {
        let b = unified(&[80, 0x04, 0, 0]).unwrap();
        assert_eq!((b.percent, b.approximate, b.charging), (80, false, Charging::Discharging));
        let b = unified(&[0, 0x02, 1, 1]).unwrap();
        assert_eq!((b.percent, b.approximate, b.charging), (20, true, Charging::Charging));
    }

    #[test]
    fn voltage_curve() {
        assert_eq!(percent_from_millivolts(4200), 100);
        assert_eq!(percent_from_millivolts(3811), 50);
        assert_eq!(percent_from_millivolts(3400), 0);
        let mid = percent_from_millivolts(3890);
        assert!((60..=70).contains(&mid));
    }
}
