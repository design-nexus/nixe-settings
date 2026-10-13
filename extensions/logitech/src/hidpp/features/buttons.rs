//! Reprogrammable controls (0x1B04): list a device's buttons and remap them to
//! other controls in the same group. Remaps run on the device itself, so they
//! keep working without this app running.

use crate::hidpp::device::{Info, be16, feature as f, need};
use crate::hidpp::link::{Link, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Control {
    pub cid: u16,
    pub flags: u8,
    pub group: u8,
    pub group_mask: u8,
    /// What the control currently does (its own cid when not remapped).
    pub remap: u16,
}

impl Control {
    pub fn reprogrammable(&self) -> bool {
        self.flags & 0x10 != 0
    }

    pub fn is_virtual(&self) -> bool {
        self.flags & 0x80 != 0
    }

    /// Controls this one can be remapped to, including itself.
    pub fn targets<'a>(&self, all: &'a [Control]) -> Vec<&'a Control> {
        all.iter()
            .filter(|c| c.cid == self.cid || (c.group > 0 && c.group <= 8 && self.group_mask & (1 << (c.group - 1)) != 0))
            .collect()
    }
}

pub fn name(cid: u16) -> String {
    let known = match cid {
        0x0050 => "Left click",
        0x0051 => "Right click",
        0x0052 => "Middle click",
        0x0053 => "Back",
        0x0056 => "Forward",
        0x0059 => "Tilt left",
        0x005A => "Tilt right",
        0x005B => "Scroll left",
        0x005D => "Scroll right",
        0x0060 => "Gesture button",
        0x00C3 => "Gesture button",
        0x00C4 => "Wheel mode shift",
        0x00D7 => "Virtual gesture",
        0x00ED => "DPI switch",
        0x00FD => "DPI switch",
        0x0001 => "Volume up",
        0x0002 => "Volume down",
        0x0003 => "Mute",
        0x0004 => "Play/pause",
        0x0005 => "Next track",
        0x0006 => "Previous track",
        0x0007 => "Stop",
        0x000A => "Calculator",
        0x006F => "Lock screen",
        0x00C7 => "Brightness down",
        0x00C8 => "Brightness up",
        0x00D0 => "Show desktop",
        0x00D1 => "App switcher",
        0x00D4 => "Emoji",
        0x00E0 => "Dictation",
        0x00E1 => "Screen capture",
        0x00E4 => "Mic mute",
        0x0111 => "Backlight down",
        0x0112 => "Backlight up",
        0x0113 => "Previous",
        0x0114 => "Play/pause",
        0x0115 => "Next",
        0x0116 => "Mute",
        0x0117 => "Volume down",
        0x0118 => "Volume up",
        0x0119 => "App contextual menu",
        0x0120 => "Screen capture",
        0x0103 => "Snipping tool",
        0x00E3 => "Search",
        _ => "",
    };
    if known.is_empty() { format!("Control {cid:#06x}") } else { known.to_string() }
}

pub fn read(link: &mut Link, info: &Info) -> Result<Vec<Control>> {
    let count = need(&info.call(link, f::REPROG_CONTROLS_V4, 0, &[])?, 1)?[0];
    let mut controls = Vec::new();
    for i in 0..count {
        let r = info.call(link, f::REPROG_CONTROLS_V4, 1, &[i])?;
        let r = need(&r, 8)?;
        let cid = be16(r, 0);
        let rep = info.call(link, f::REPROG_CONTROLS_V4, 2, &cid.to_be_bytes())?;
        let rep = need(&rep, 5)?;
        let remap = be16(rep, 3);
        controls.push(Control { cid, flags: r[4], group: r[6], group_mask: r[7], remap: if remap == 0 { cid } else { remap } });
    }
    Ok(controls)
}

/// Make `cid` act like `target` (pass `cid` itself to restore it).
pub fn remap(link: &mut Link, info: &Info, cid: u16, target: u16) -> Result<()> {
    let [ch, cl] = cid.to_be_bytes();
    let [th, tl] = target.to_be_bytes();
    // Flags byte 0: leave diversion as it is.
    info.call(link, f::REPROG_CONTROLS_V4, 3, &[ch, cl, 0, th, tl]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remap_targets_follow_group_mask() {
        let all = vec![
            Control { cid: 0x50, flags: 0, group: 1, group_mask: 0, remap: 0x50 },
            Control { cid: 0x52, flags: 0x10, group: 3, group_mask: 0x06, remap: 0x52 },
            Control { cid: 0x53, flags: 0x10, group: 2, group_mask: 0x06, remap: 0x53 },
            Control { cid: 0x56, flags: 0x10, group: 3, group_mask: 0x06, remap: 0x56 },
        ];
        let t: Vec<u16> = all[2].targets(&all).iter().map(|c| c.cid).collect();
        assert_eq!(t, vec![0x52, 0x53, 0x56]);
        assert_eq!(name(0x53), "Back");
    }
}
