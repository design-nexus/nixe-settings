//! Gaming features: onboard profiles (0x8100) and LED zones (0x8070).

use crate::hidpp::device::{Info, be16, feature as f, need};
use crate::hidpp::link::{Link, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Onboard {
    pub profiles: u8,
    /// The mouse runs from its stored profiles rather than from the computer.
    pub onboard: bool,
    /// 1-based.
    pub current: u8,
}

pub fn read_onboard(link: &mut Link, info: &Info) -> Result<Onboard> {
    let d = info.call(link, f::ONBOARD_PROFILES, 0, &[])?;
    let profiles = need(&d, 4)?[3];
    let mode = need(&info.call(link, f::ONBOARD_PROFILES, 2, &[])?, 1)?[0];
    let cur = info.call(link, f::ONBOARD_PROFILES, 4, &[])?;
    Ok(Onboard { profiles, onboard: mode == 1, current: be16(need(&cur, 2)?, 0) as u8 })
}

pub fn write_onboard_mode(link: &mut Link, info: &Info, onboard: bool) -> Result<()> {
    info.call(link, f::ONBOARD_PROFILES, 1, &[if onboard { 1 } else { 2 }]).map(|_| ())
}

pub fn write_profile(link: &mut Link, info: &Info, profile: u8) -> Result<()> {
    info.call(link, f::ONBOARD_PROFILES, 3, &[0, profile]).map(|_| ())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Effect {
    Off,
    Static,
    Breathe,
    Cycle,
}

impl Effect {
    pub const ALL: [Effect; 4] = [Effect::Static, Effect::Breathe, Effect::Cycle, Effect::Off];

    fn id(self) -> u16 {
        match self {
            Effect::Off => 0x00,
            Effect::Static => 0x01,
            Effect::Cycle => 0x03,
            Effect::Breathe => 0x0A,
        }
    }

    fn from_id(id: u16) -> Option<Self> {
        Effect::ALL.into_iter().find(|e| e.id() == id)
    }

    pub fn key(self) -> &'static str {
        match self {
            Effect::Off => "off",
            Effect::Static => "static",
            Effect::Breathe => "breathe",
            Effect::Cycle => "cycle",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Effect::ALL.into_iter().find(|e| e.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            Effect::Off => "Off",
            Effect::Static => "Solid",
            Effect::Breathe => "Breathe",
            Effect::Cycle => "Colour cycle",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zone {
    pub index: u8,
    pub location: u16,
    /// Effects this zone supports, with the device's index for each.
    pub effects: Vec<(Effect, u8)>,
    pub effect: Effect,
    pub colour: [u8; 3],
    /// Breathe/cycle period in ms.
    pub period: u16,
}

impl Zone {
    pub fn name(&self) -> String {
        match self.location {
            1 => "Main".into(),
            2 => "Logo".into(),
            3 => "Left side".into(),
            4 => "Right side".into(),
            5 => "All zones".into(),
            n => format!("Zone {n}"),
        }
    }
}

pub fn read_zones(link: &mut Link, info: &Info) -> Result<Vec<Zone>> {
    let count = need(&info.call(link, f::COLOR_LED_EFFECTS, 0, &[])?, 1)?[0];
    let mut zones = Vec::new();
    for z in 0..count {
        let zi = info.call(link, f::COLOR_LED_EFFECTS, 1, &[z])?;
        let zi = need(&zi, 4)?;
        let location = be16(zi, 1);
        let mut effects = Vec::new();
        for e in 0..zi[3] {
            let r = info.call(link, f::COLOR_LED_EFFECTS, 2, &[z, e])?;
            if let Some(effect) = Effect::from_id(be16(need(&r, 4)?, 2)) {
                effects.push((effect, e));
            }
        }
        // Current effect: fn 0xE (getZoneEffect) → zone, effect index, params.
        let (mut effect, mut colour, mut period) = (Effect::Static, [255, 255, 255], 3000);
        if let Ok(cur) = info.call(link, f::COLOR_LED_EFFECTS, 0x0E, &[z])
            && cur.len() >= 12
        {
            let idx = cur[1];
            effect = effects.iter().find(|(_, i)| *i == idx).map(|(e, _)| *e).unwrap_or(Effect::Static);
            let p = &cur[2..12];
            match effect {
                Effect::Static => colour = [p[0], p[1], p[2]],
                Effect::Breathe => {
                    colour = [p[0], p[1], p[2]];
                    period = be16(p, 3);
                }
                Effect::Cycle => period = be16(p, 5),
                Effect::Off => {}
            }
        }
        zones.push(Zone { index: z, location, effects, effect, colour, period: period.max(500) });
    }
    Ok(zones)
}

/// Set a zone's effect. Colour applies to solid and breathe; period to breathe and cycle.
pub fn write_zone(link: &mut Link, info: &Info, zone: &Zone, effect: Effect, colour: [u8; 3], period: u16) -> Result<()> {
    let Some((_, idx)) = zone.effects.iter().find(|(e, _)| *e == effect) else {
        return Err(crate::hidpp::link::Error::V20(0x02));
    };
    let mut p = [0u8; 10];
    let [ph, pl] = period.to_be_bytes();
    match effect {
        Effect::Static => p[..3].copy_from_slice(&colour),
        Effect::Breathe => {
            p[..3].copy_from_slice(&colour);
            p[3] = ph;
            p[4] = pl;
            p[6] = 100;
        }
        Effect::Cycle => {
            p[5] = ph;
            p[6] = pl;
            p[7] = 100;
        }
        Effect::Off => {}
    }
    let mut params = vec![zone.index, *idx];
    params.extend_from_slice(&p);
    params.push(0x01);
    info.call(link, f::COLOR_LED_EFFECTS, 3, &params).map(|_| ())
}
