//! Controllers through the kernel's input devices: finding them, reading
//! what's held and where the sticks are (ioctls on `/dev/input/eventN`, which
//! udev already opens to the signed-in user for game controllers), and
//! playing a rumble through force feedback.

use anyhow::{Context, Result, bail};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Pad {
    pub event: PathBuf,
    pub name: String,
    pub vendor: u32,
    pub product: u32,
    pub bluetooth: bool,
    pub rumble: bool,
    /// The HID device it belongs to, for finding its battery and lights.
    pub hid: Option<PathBuf>,
    uniq: String,
}

impl Pad {
    pub fn id(&self) -> String {
        let key = if self.uniq.is_empty() { self.event.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default() } else { self.uniq.replace(':', "") };
        format!("{:04x}{:04x}-{}", self.vendor, self.product, key.to_lowercase())
    }

    pub fn title(&self) -> String {
        let n = self.name.replace("Sony Interactive Entertainment ", "").replace("Microsoft X-Box", "Xbox");
        if n.contains("Wireless Controller") && self.vendor == 0x054c {
            return match self.product {
                0x0ce6 | 0x0df2 => "DualSense".into(),
                0x05c4 | 0x09cc => "DualShock 4".into(),
                _ => n,
            };
        }
        n
    }
}

fn read(p: PathBuf) -> String {
    std::fs::read_to_string(p).map(|s| s.trim().to_string()).unwrap_or_default()
}

fn hex(p: PathBuf) -> u32 {
    u32::from_str_radix(&read(p), 16).unwrap_or(0)
}

/// Whether udev marked the device a joystick (ID_INPUT_JOYSTICK).
fn is_joystick(event: &str) -> bool {
    let dev = read(PathBuf::from(format!("/sys/class/input/{event}/dev")));
    let Some((maj, min)) = dev.split_once(':') else { return false };
    std::fs::read_to_string(format!("/run/udev/data/c{maj}:{min}")).is_ok_and(|t| t.lines().any(|l| l == "E:ID_INPUT_JOYSTICK=1"))
}

pub fn pads() -> Vec<Pad> {
    let Ok(rd) = std::fs::read_dir("/sys/class/input") else { return Vec::new() };
    let mut out: Vec<Pad> = rd
        .flatten()
        .filter_map(|e| {
            let event = e.file_name().to_string_lossy().to_string();
            if !event.starts_with("event") || !is_joystick(&event) {
                return None;
            }
            let base = e.path().join("device");
            let ff = read(base.join("capabilities/ff"));
            Some(Pad {
                event: PathBuf::from("/dev/input").join(&event),
                name: read(base.join("name")),
                vendor: hex(base.join("id/vendor")),
                product: hex(base.join("id/product")),
                bluetooth: hex(base.join("id/bustype")) == 0x05,
                rumble: !ff.is_empty() && ff.split_whitespace().any(|w| w != "0"),
                hid: std::fs::canonicalize(base.join("device")).ok(),
                uniq: read(base.join("uniq")),
            })
        })
        .collect();
    out.sort_by(|a, b| a.event.cmp(&b.event));
    out
}

// ---------- Reading the state ----------

const fn ioc(dir: u64, nr: u64, size: u64) -> u64 {
    (dir << 30) | (size << 16) | (0x45 << 8) | nr
}
const READ: u64 = 2;
const WRITE: u64 = 1;

#[repr(C)]
#[derive(Default)]
struct AbsInfo {
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}

pub struct State {
    pub buttons: Vec<String>,
    pub left: Option<(f64, f64)>,
    pub right: Option<(f64, f64)>,
    pub lt: Option<f64>,
    pub rt: Option<f64>,
    pub dpad: Option<(f64, f64)>,
}

/// Button codes and the names shown, Xbox / PlayStation.
const BUTTONS: &[(usize, &str)] = &[
    (0x130, "A / ✕"),
    (0x131, "B / ○"),
    (0x133, "Y / △"),
    (0x134, "X / □"),
    (0x136, "LB / L1"),
    (0x137, "RB / R1"),
    (0x138, "LT / L2"),
    (0x139, "RT / R2"),
    (0x13a, "View / Share"),
    (0x13b, "Menu / Options"),
    (0x13c, "Home"),
    (0x13d, "Left stick press"),
    (0x13e, "Right stick press"),
    (0x220, "D-pad up"),
    (0x221, "D-pad down"),
    (0x222, "D-pad left"),
    (0x223, "D-pad right"),
];

fn abs(fd: i32, code: u64) -> Option<AbsInfo> {
    let mut a = AbsInfo::default();
    // SAFETY: EVIOCGABS fills an input_absinfo, which AbsInfo matches.
    let r = unsafe { libc::ioctl(fd, ioc(READ, 0x40 + code, 24) as _, &mut a as *mut AbsInfo) };
    (r >= 0 && a.maximum > a.minimum).then_some(a)
}

fn centred(a: &AbsInfo) -> f64 {
    let mid = (a.maximum as f64 + a.minimum as f64) / 2.0;
    let half = (a.maximum as f64 - a.minimum as f64) / 2.0;
    ((a.value as f64 - mid) / half).clamp(-1.0, 1.0)
}

fn fraction(a: &AbsInfo) -> f64 {
    ((a.value - a.minimum) as f64 / (a.maximum - a.minimum) as f64).clamp(0.0, 1.0)
}

pub fn state(pad: &Pad) -> Result<State> {
    let f = OpenOptions::new().read(true).open(&pad.event).context("no permission to read it")?;
    let fd = f.as_raw_fd();
    let mut keys = [0u8; 96];
    // SAFETY: EVIOCGKEY writes at most `len` bytes into the buffer.
    let r = unsafe { libc::ioctl(fd, ioc(READ, 0x18, keys.len() as u64) as _, keys.as_mut_ptr()) };
    if r < 0 {
        bail!("it didn't answer");
    }
    let held = |code: usize| keys.get(code / 8).is_some_and(|b| b & (1 << (code % 8)) != 0);
    let buttons = BUTTONS.iter().filter(|(c, _)| held(*c)).map(|(_, n)| n.to_string()).collect();
    let pair = |x: u64, y: u64| Some((centred(&abs(fd, x)?), centred(&abs(fd, y)?)));
    Ok(State {
        buttons,
        left: pair(0x00, 0x01),
        right: pair(0x03, 0x04),
        lt: abs(fd, 0x02).map(|a| fraction(&a)),
        rt: abs(fd, 0x05).map(|a| fraction(&a)),
        dpad: pair(0x10, 0x11),
    })
}

// ---------- Rumble ----------

/// struct ff_effect on 64-bit Linux: 48 bytes, the union at 16.
fn rumble_effect(strong: u16, weak: u16, ms: u16) -> [u8; 48] {
    let mut e = [0u8; 48];
    e[0..2].copy_from_slice(&0x50u16.to_ne_bytes()); // FF_RUMBLE
    e[2..4].copy_from_slice(&(-1i16).to_ne_bytes()); // a new effect
    e[10..12].copy_from_slice(&ms.to_ne_bytes()); // replay.length
    e[16..18].copy_from_slice(&strong.to_ne_bytes());
    e[18..20].copy_from_slice(&weak.to_ne_bytes());
    e
}

/// Rumble for `ms` milliseconds.
pub fn rumble(pad: &Pad, strong: u16, weak: u16, ms: u16) -> Result<()> {
    let mut f = OpenOptions::new().read(true).write(true).open(&pad.event).context("no permission to make it rumble")?;
    let fd = f.as_raw_fd();
    let mut effect = rumble_effect(strong, weak, ms);
    // SAFETY: EVIOCSFF reads and updates an ff_effect, which `effect` is laid out as.
    let r = unsafe { libc::ioctl(fd, ioc(WRITE, 0x80, 48) as _, effect.as_mut_ptr()) };
    if r < 0 {
        bail!("it refused the rumble");
    }
    let id = i16::from_ne_bytes([effect[2], effect[3]]);
    // struct input_event: a 16-byte time, then EV_FF, the effect, and 1 (play).
    let mut ev = [0u8; 24];
    ev[16..18].copy_from_slice(&0x15u16.to_ne_bytes());
    ev[18..20].copy_from_slice(&(id as u16).to_ne_bytes());
    ev[20..24].copy_from_slice(&1i32.to_ne_bytes());
    f.write_all(&ev)?;
    std::thread::sleep(std::time::Duration::from_millis(ms as u64 + 50));
    // SAFETY: EVIOCRMFF takes the effect id by value.
    unsafe { libc::ioctl(fd, ioc(WRITE, 0x81, 4) as _, id as libc::c_long) };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_numbers() {
        // EVIOCGKEY(96), EVIOCGABS(0), EVIOCSFF, EVIOCRMFF as the kernel headers give them.
        assert_eq!(ioc(READ, 0x18, 96), 0x8060_4518);
        assert_eq!(ioc(READ, 0x40, 24), 0x8018_4540);
        assert_eq!(ioc(WRITE, 0x80, 48), 0x4030_4580);
        assert_eq!(ioc(WRITE, 0x81, 4), 0x4004_4581);
    }

    #[test]
    fn rumble_layout() {
        let e = rumble_effect(0x1234, 0x5678, 600);
        assert_eq!(&e[0..2], &0x50u16.to_ne_bytes());
        assert_eq!(&e[2..4], &(-1i16).to_ne_bytes());
        assert_eq!(&e[10..12], &600u16.to_ne_bytes());
        assert_eq!(&e[16..20], &[0x34, 0x12, 0x78, 0x56]);
    }

    #[test]
    fn axes() {
        let a = AbsInfo { value: 0, minimum: -32768, maximum: 32767, ..Default::default() };
        assert!(centred(&a).abs() < 0.001);
        let t = AbsInfo { value: 255, minimum: 0, maximum: 255, ..Default::default() };
        assert_eq!(fraction(&t), 1.0);
    }
}
