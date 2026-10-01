//! Pointer speed: AdjustableDPI (0x2201) and report rate (0x8060 / 0x8061).

use crate::hidpp::device::{Info, be16, feature as f, need};
use crate::hidpp::link::{Error, Link, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum DpiChoices {
    List(Vec<u16>),
    Range { min: u16, max: u16, step: u16 },
}

impl DpiChoices {
    pub fn bounds(&self) -> (u16, u16, u16) {
        match self {
            DpiChoices::Range { min, max, step } => (*min, *max, *step),
            DpiChoices::List(v) => {
                let min = v.iter().copied().min().unwrap_or(400);
                let max = v.iter().copied().max().unwrap_or(min);
                let step = v.windows(2).map(|w| w[1].abs_diff(w[0])).filter(|d| *d > 0).min().unwrap_or(50);
                (min, max, step)
            }
        }
    }

    /// The nearest DPI the sensor actually supports.
    pub fn snap(&self, dpi: u16) -> u16 {
        match self {
            DpiChoices::Range { min, max, step } => {
                let step = (*step).max(1);
                let v = dpi.clamp(*min, *max);
                let n = ((v - min) as f32 / step as f32).round() as u16;
                (min + n * step).min(*max)
            }
            DpiChoices::List(v) => v.iter().copied().min_by_key(|x| x.abs_diff(dpi)).unwrap_or(dpi),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Dpi {
    pub current: u16,
    pub default: u16,
    pub choices: DpiChoices,
}

/// Parse the sensor DPI list: values, or `min, 0xE000|step, max` ranges, ending in 0.
pub fn parse_dpi_list(r: &[u8]) -> DpiChoices {
    let words: Vec<u16> = r.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).take_while(|w| *w != 0).collect();
    if let Some(i) = words.iter().position(|w| *w >= 0xE000)
        && i > 0
        && i + 1 < words.len()
    {
        return DpiChoices::Range { min: words[i - 1], max: words[i + 1], step: words[i] & 0x1FFF };
    }
    DpiChoices::List(words)
}

pub fn read_dpi(link: &mut Link, info: &Info) -> Result<Dpi> {
    let list = info.call(link, f::ADJUSTABLE_DPI, 1, &[0])?;
    let choices = parse_dpi_list(&need(&list, 1)?[1..]);
    let r = info.call(link, f::ADJUSTABLE_DPI, 2, &[0])?;
    let r = need(&r, 5)?;
    let current = be16(r, 1);
    let default = be16(r, 3);
    Ok(Dpi { current, default: if default == 0 { current } else { default }, choices })
}

pub fn write_dpi(link: &mut Link, info: &Info, dpi: u16) -> Result<()> {
    let [hi, lo] = dpi.to_be_bytes();
    info.call(link, f::ADJUSTABLE_DPI, 3, &[0, hi, lo]).map(|_| ())
}

/// Report rates, in Hz.
#[derive(Debug, Clone, PartialEq)]
pub struct ReportRate {
    pub current: u32,
    pub choices: Vec<u32>,
}

const EXTENDED_RATES: [u32; 7] = [125, 250, 500, 1000, 2000, 4000, 8000];

pub fn read_report_rate(link: &mut Link, info: &Info) -> Result<ReportRate> {
    if info.has(f::EXTENDED_REPORT_RATE) {
        let list = info.call(link, f::EXTENDED_REPORT_RATE, 1, &[])?;
        let mask = be16(need(&list, 2)?, 0);
        let choices = EXTENDED_RATES.iter().enumerate().filter(|(i, _)| mask & (1 << i) != 0).map(|(_, hz)| *hz).collect();
        let cur = need(&info.call(link, f::EXTENDED_REPORT_RATE, 2, &[])?, 1)?[0] as usize;
        return Ok(ReportRate { current: EXTENDED_RATES.get(cur).copied().unwrap_or(1000), choices });
    }
    let list = info.call(link, f::REPORT_RATE, 0, &[])?;
    let mask = need(&list, 1)?[0];
    let mut choices: Vec<u32> = (0..8).filter(|b| mask & (1 << b) != 0).map(|b| 1000 / (b + 1)).collect();
    choices.sort_unstable();
    let ms = need(&info.call(link, f::REPORT_RATE, 1, &[])?, 1)?[0].max(1) as u32;
    Ok(ReportRate { current: 1000 / ms, choices })
}

pub fn write_report_rate(link: &mut Link, info: &Info, hz: u32) -> Result<()> {
    if info.has(f::EXTENDED_REPORT_RATE) {
        let i = EXTENDED_RATES.iter().position(|r| *r == hz).ok_or(Error::V20(0x02))?;
        return info.call(link, f::EXTENDED_REPORT_RATE, 3, &[i as u8]).map(|_| ());
    }
    let ms = (1000 / hz.max(1)).clamp(1, 8) as u8;
    info.call(link, f::REPORT_RATE, 2, &[ms]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpi_range() {
        // 200, step 50, 8000, end.
        let r = [0x00, 0xC8, 0xE0, 0x32, 0x1F, 0x40, 0, 0];
        let c = parse_dpi_list(&r);
        assert_eq!(c, DpiChoices::Range { min: 200, max: 8000, step: 50 });
        assert_eq!(c.snap(1234), 1250);
        assert_eq!(c.snap(60000), 8000);
    }

    #[test]
    fn dpi_list() {
        let r = [0x01, 0x90, 0x03, 0x20, 0x06, 0x40, 0, 0];
        let c = parse_dpi_list(&r);
        assert_eq!(c, DpiChoices::List(vec![400, 800, 1600]));
        assert_eq!(c.bounds(), (400, 1600, 400));
        assert_eq!(c.snap(1000), 800);
    }
}
