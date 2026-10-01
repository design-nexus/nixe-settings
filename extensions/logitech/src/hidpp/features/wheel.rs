//! Scroll wheels: SmartShift (0x2110 / 0x2111), hi-res wheel (0x2121) and
//! thumb wheel (0x2150).

use crate::hidpp::device::{Info, feature as f, need};
use crate::hidpp::link::{Link, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelMode {
    /// Always clicks.
    Ratchet,
    /// Always spins freely.
    FreeSpin,
    /// Clicks, and lets go into free spin past this speed (1 = easiest, 254 = hardest).
    SmartShift(u8),
}

impl WheelMode {
    pub fn id(self) -> &'static str {
        match self {
            WheelMode::Ratchet => "ratchet",
            WheelMode::FreeSpin => "free",
            WheelMode::SmartShift(_) => "smart",
        }
    }
}

fn smart_shift_feature(info: &Info) -> Option<u16> {
    info.first_of(&[f::SMART_SHIFT_ENHANCED, f::SMART_SHIFT])
}

pub fn parse_mode(mode: u8, threshold: u8) -> WheelMode {
    match (mode, threshold) {
        (1, _) => WheelMode::FreeSpin,
        (_, 0xFF) | (_, 0) => WheelMode::Ratchet,
        (_, t) => WheelMode::SmartShift(t),
    }
}

pub fn read_mode(link: &mut Link, info: &Info) -> Result<WheelMode> {
    let feat = smart_shift_feature(info).unwrap_or(f::SMART_SHIFT);
    let func = if feat == f::SMART_SHIFT_ENHANCED { 1 } else { 0 };
    let r = info.call(link, feat, func, &[])?;
    let r = need(&r, 2)?;
    Ok(parse_mode(r[0], r[1]))
}

pub fn write_mode(link: &mut Link, info: &Info, mode: WheelMode) -> Result<()> {
    let feat = smart_shift_feature(info).unwrap_or(f::SMART_SHIFT);
    let func = if feat == f::SMART_SHIFT_ENHANCED { 2 } else { 1 };
    let params = match mode {
        WheelMode::FreeSpin => [1, 0, 0],
        WheelMode::Ratchet => [2, 0xFF, 0],
        WheelMode::SmartShift(t) => [2, t.clamp(1, 254), 0],
    };
    info.call(link, feat, func, &params).map(|_| ())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HiRes {
    pub can_invert: bool,
    pub hires: bool,
    pub inverted: bool,
}

pub fn read_hires(link: &mut Link, info: &Info) -> Result<HiRes> {
    let caps = info.call(link, f::HIRES_WHEEL, 0, &[])?;
    let caps = need(&caps, 2)?;
    let mode = need(&info.call(link, f::HIRES_WHEEL, 1, &[])?, 1)?[0];
    Ok(HiRes { can_invert: caps[1] & 0x08 != 0, hires: mode & 0x02 != 0, inverted: mode & 0x04 != 0 })
}

pub fn write_hires(link: &mut Link, info: &Info, hires: bool, inverted: bool) -> Result<()> {
    // Keep the wheel reporting to the OS (bit 0 clear), never diverted to us.
    let mode = (if hires { 0x02 } else { 0 }) | (if inverted { 0x04 } else { 0 });
    info.call(link, f::HIRES_WHEEL, 2, &[mode]).map(|_| ())
}

pub fn read_thumb_inverted(link: &mut Link, info: &Info) -> Result<bool> {
    let r = info.call(link, f::THUMB_WHEEL, 1, &[])?;
    Ok(need(&r, 2)?[1] & 0x01 != 0)
}

pub fn write_thumb_inverted(link: &mut Link, info: &Info, inverted: bool) -> Result<()> {
    info.call(link, f::THUMB_WHEEL, 2, &[0, inverted as u8]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes() {
        assert_eq!(parse_mode(1, 10), WheelMode::FreeSpin);
        assert_eq!(parse_mode(2, 0xFF), WheelMode::Ratchet);
        assert_eq!(parse_mode(2, 12), WheelMode::SmartShift(12));
    }
}
