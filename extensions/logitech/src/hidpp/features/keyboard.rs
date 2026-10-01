//! Keyboard features: Fn inversion (0x40A0 / 0x40A2 / 0x40A3), disable keys
//! (0x4521), Easy-Switch (0x1814) and backlight (0x1982).

use crate::hidpp::device::{Info, be16, feature as f, need};
use crate::hidpp::link::{Error, Link, Result};

fn fn_feature(info: &Info) -> Option<u16> {
    info.first_of(&[f::K375S_FN_INVERSION, f::NEW_FN_INVERSION, f::FN_INVERSION])
}

pub fn has_fn_swap(info: &Info) -> bool {
    fn_feature(info).is_some()
}

/// `true` when the top row sends media keys and Fn gives F1–F12.
pub fn read_fn_swap(link: &mut Link, info: &Info) -> Result<bool> {
    let feat = fn_feature(info).ok_or(Error::NoFeature(f::FN_INVERSION))?;
    let r = info.call(link, feat, 0, &[])?;
    let at = if feat == f::K375S_FN_INVERSION { 1 } else { 0 };
    Ok(need(&r, at + 1)?[at] & 0x01 != 0)
}

pub fn write_fn_swap(link: &mut Link, info: &Info, media_first: bool) -> Result<()> {
    let feat = fn_feature(info).ok_or(Error::NoFeature(f::FN_INVERSION))?;
    let v = media_first as u8;
    let params: &[u8] = if feat == f::K375S_FN_INVERSION { &[0xFF, v] } else { &[v] };
    info.call(link, feat, 1, params).map(|_| ())
}

pub const DISABLE_KEY_NAMES: [&str; 5] = ["Caps Lock", "Num Lock", "Scroll Lock", "Insert", "Super"];

/// (keys the keyboard can disable, keys disabled now) as bit masks.
pub fn read_disabled_keys(link: &mut Link, info: &Info) -> Result<(u8, u8)> {
    let caps = need(&info.call(link, f::DISABLE_KEYS, 0, &[])?, 1)?[0];
    let now = need(&info.call(link, f::DISABLE_KEYS, 1, &[])?, 1)?[0];
    Ok((caps, now))
}

pub fn write_disabled_keys(link: &mut Link, info: &Info, mask: u8) -> Result<()> {
    info.call(link, f::DISABLE_KEYS, 2, &[mask]).map(|_| ())
}

/// (number of hosts, current host, 0-based).
pub fn read_hosts(link: &mut Link, info: &Info) -> Result<(u8, u8)> {
    let r = info.call(link, f::CHANGE_HOST, 0, &[])?;
    let r = need(&r, 2)?;
    Ok((r[0], r[1]))
}

/// Switch to another host. The device leaves this computer right away, so it
/// usually never answers; a timeout here is success.
pub fn switch_host(link: &mut Link, info: &Info, host: u8) -> Result<()> {
    match info.call(link, f::CHANGE_HOST, 1, &[host]) {
        Ok(_) | Err(Error::Timeout) => Ok(()),
        Err(e) => Err(e),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backlight {
    pub enabled: bool,
    options: u8,
    pub level: u8,
    /// Number of manual levels (0 when the keyboard only does automatic).
    pub levels: u8,
    /// How long the light stays on after you stop typing, in seconds.
    pub timeout: u16,
    dhi: u16,
    dpow: u16,
}

const MANUAL: u8 = 0x03 << 3;

pub fn read_backlight(link: &mut Link, info: &Info) -> Result<Backlight> {
    let r = info.call(link, f::BACKLIGHT2, 0, &[])?;
    let r = need(&r, 12)?;
    let levels = info.call(link, f::BACKLIGHT2, 2, &[]).ok().and_then(|l| l.first().copied()).unwrap_or(0);
    Ok(Backlight {
        enabled: r[0] != 0,
        options: r[1],
        level: r[5],
        levels,
        timeout: be16(r, 6) * 5,
        dhi: be16(r, 8),
        dpow: be16(r, 10),
    })
}

pub fn write_backlight(link: &mut Link, info: &Info, b: &Backlight, manual_level: bool) -> Result<()> {
    let options = if manual_level { (b.options & !(0x03 << 3)) | MANUAL } else { b.options };
    let [th, tl] = (b.timeout / 5).max(1).to_be_bytes();
    let [hh, hl] = b.dhi.to_be_bytes();
    let [ph, pl] = b.dpow.to_be_bytes();
    let params = [b.enabled as u8, options, 0xFF, b.level, th, tl, hh, hl, ph, pl];
    info.call(link, f::BACKLIGHT2, 1, &params).map(|_| ())
}
