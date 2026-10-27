//! A client for OpenRGB's SDK server (TCP, port 6742), which runs the RGB
//! lighting of keyboards, mice, RAM, fans, motherboards and graphics cards
//! from many brands. Protocol version 3: devices, their modes (effects) with
//! speed, brightness and colours, zones and LEDs, and saved profiles.
//!
//! Every packet: "ORGB", device index (u32), packet id (u32), body size
//! (u32), then the body; all little-endian. Strings are a u16 length that
//! includes a closing NUL, then the bytes.

use anyhow::{Context, Result, bail};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

pub const PORT: u16 = 6742;
const PROTOCOL: u32 = 3;

const REQUEST_CONTROLLER_COUNT: u32 = 0;
const REQUEST_CONTROLLER_DATA: u32 = 1;
const REQUEST_PROTOCOL_VERSION: u32 = 40;
const SET_CLIENT_NAME: u32 = 50;
const REQUEST_PROFILE_LIST: u32 = 150;
const REQUEST_SAVE_PROFILE: u32 = 151;
const REQUEST_LOAD_PROFILE: u32 = 152;
const UPDATE_LEDS: u32 = 1050;
const UPDATE_ZONE_LEDS: u32 = 1051;
const SET_CUSTOM_MODE: u32 = 1100;
const UPDATE_MODE: u32 = 1101;
const SAVE_MODE: u32 = 1102;

/// Mode flags.
pub const HAS_SPEED: u32 = 1 << 0;
pub const HAS_DIRECTION_LR: u32 = 1 << 1;
pub const HAS_DIRECTION_UD: u32 = 1 << 2;
pub const HAS_BRIGHTNESS: u32 = 1 << 4;
pub const HAS_PER_LED_COLOR: u32 = 1 << 5;
pub const HAS_MODE_SPECIFIC_COLOR: u32 = 1 << 6;
pub const HAS_RANDOM_COLOR: u32 = 1 << 7;

/// How a mode takes colour.
pub const COLOR_MODE_NONE: u32 = 0;
pub const COLOR_MODE_PER_LED: u32 = 1;
pub const COLOR_MODE_SPECIFIC: u32 = 2;
pub const COLOR_MODE_RANDOM: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    fn from_u32(v: u32) -> Rgb {
        Rgb(v as u8, (v >> 8) as u8, (v >> 16) as u8)
    }
    fn to_u32(self) -> u32 {
        self.0 as u32 | (self.1 as u32) << 8 | (self.2 as u32) << 16
    }
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
    pub fn parse(s: &str) -> Option<Rgb> {
        crate::sys::parse_hex(s).map(|(r, g, b)| Rgb(r, g, b))
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mode {
    pub name: String,
    pub value: i32,
    pub flags: u32,
    pub speed_min: u32,
    pub speed_max: u32,
    pub brightness_min: u32,
    pub brightness_max: u32,
    pub colors_min: u32,
    pub colors_max: u32,
    pub speed: u32,
    pub brightness: u32,
    pub direction: u32,
    pub color_mode: u32,
    pub colors: Vec<Rgb>,
}

impl Mode {
    pub fn has(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Zone {
    pub name: String,
    pub kind: i32,
    pub leds_count: u32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Device {
    pub index: u32,
    /// 0 motherboard, 1 RAM, 2 GPU, 3 cooler, 4 LED strip, 5 keyboard, 6 mouse,
    /// 7 mouse mat, 8 headset, 9 headset stand, 10 gamepad, 11 light, 12 speaker…
    pub kind: i32,
    pub name: String,
    pub vendor: String,
    pub description: String,
    pub version: String,
    pub serial: String,
    pub location: String,
    pub active_mode: i32,
    pub modes: Vec<Mode>,
    pub zones: Vec<Zone>,
    pub leds: Vec<String>,
    pub colors: Vec<Rgb>,
}

impl Device {
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            0 => "Motherboard",
            1 => "Memory",
            2 => "Graphics card",
            3 => "Cooler",
            4 => "LED strip",
            5 => "Keyboard",
            6 => "Mouse",
            7 => "Mouse mat",
            8 => "Headset",
            9 => "Headset stand",
            10 => "Controller",
            11 => "Light",
            12 => "Speaker",
            _ => "Device",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self.kind {
            5 => "input-keyboard-symbolic",
            6 | 7 => "input-mouse-symbolic",
            8 | 9 => "audio-headset-symbolic",
            10 => "input-gaming-symbolic",
            12 => "audio-speakers-symbolic",
            1 | 2 | 0 => "computer-symbolic",
            _ => "weather-clear-symbolic",
        }
    }

    pub fn mode(&self) -> Option<&Mode> {
        usize::try_from(self.active_mode).ok().and_then(|i| self.modes.get(i))
    }

    /// A stable id for pages: the vendor and name and the location, made safe.
    pub fn id(&self) -> String {
        let raw = format!("{}-{}-{}", self.name, self.serial, self.location);
        let mut s: String = raw.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        while s.contains("--") {
            s = s.replace("--", "-");
        }
        s.trim_matches('-').chars().take(60).collect()
    }
}

// ---------- Reading ----------

struct Cursor<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.at + n > self.b.len() {
            bail!("OpenRGB sent less than expected");
        }
        let s = &self.b[self.at..self.at + n];
        self.at += n;
        Ok(s)
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn string(&mut self) -> Result<String> {
        let n = self.u16()? as usize;
        let raw = self.take(n)?;
        Ok(String::from_utf8_lossy(raw.strip_suffix(&[0]).unwrap_or(raw)).to_string())
    }
}

fn parse_mode(c: &mut Cursor) -> Result<Mode> {
    let mut m = Mode { name: c.string()?, value: c.i32()?, flags: c.u32()?, ..Default::default() };
    m.speed_min = c.u32()?;
    m.speed_max = c.u32()?;
    m.brightness_min = c.u32()?;
    m.brightness_max = c.u32()?;
    m.colors_min = c.u32()?;
    m.colors_max = c.u32()?;
    m.speed = c.u32()?;
    m.brightness = c.u32()?;
    m.direction = c.u32()?;
    m.color_mode = c.u32()?;
    let n = c.u16()?;
    for _ in 0..n {
        m.colors.push(Rgb::from_u32(c.u32()?));
    }
    Ok(m)
}

/// The body of a controller-data reply (protocol 3).
pub fn parse_device(index: u32, body: &[u8]) -> Result<Device> {
    let mut c = Cursor { b: body, at: 0 };
    let _size = c.u32()?;
    let mut d = Device { index, kind: c.i32()?, name: c.string()?, ..Default::default() };
    d.vendor = c.string()?;
    d.description = c.string()?;
    d.version = c.string()?;
    d.serial = c.string()?;
    d.location = c.string()?;
    let nmodes = c.u16()?;
    d.active_mode = c.i32()?;
    for _ in 0..nmodes {
        d.modes.push(parse_mode(&mut c)?);
    }
    let nzones = c.u16()?;
    for _ in 0..nzones {
        let name = c.string()?;
        let kind = c.i32()?;
        let _min = c.u32()?;
        let _max = c.u32()?;
        let count = c.u32()?;
        let matrix = c.u16()? as usize;
        if matrix > 0 {
            c.take(matrix)?;
        }
        d.zones.push(Zone { name, kind, leds_count: count });
    }
    let nleds = c.u16()?;
    for _ in 0..nleds {
        d.leds.push(c.string()?);
        c.u32()?;
    }
    let ncolors = c.u16()?;
    for _ in 0..ncolors {
        d.colors.push(Rgb::from_u32(c.u32()?));
    }
    Ok(d)
}

// ---------- Writing ----------

fn put_string(out: &mut Vec<u8>, s: &str) {
    out.extend(((s.len() + 1) as u16).to_le_bytes());
    out.extend(s.as_bytes());
    out.push(0);
}

/// A mode as UPDATE_MODE wants it.
pub fn mode_body(index: i32, m: &Mode) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(index.to_le_bytes());
    put_string(&mut b, &m.name);
    b.extend(m.value.to_le_bytes());
    for v in [m.flags, m.speed_min, m.speed_max, m.brightness_min, m.brightness_max, m.colors_min, m.colors_max, m.speed, m.brightness, m.direction, m.color_mode] {
        b.extend(v.to_le_bytes());
    }
    b.extend((m.colors.len() as u16).to_le_bytes());
    for c in &m.colors {
        b.extend(c.to_u32().to_le_bytes());
    }
    let mut out = ((b.len() + 4) as u32).to_le_bytes().to_vec();
    out.extend(b);
    out
}

fn colors_body(prefix: &[u8], colors: &[Rgb]) -> Vec<u8> {
    let mut b = prefix.to_vec();
    b.extend((colors.len() as u16).to_le_bytes());
    for c in colors {
        b.extend(c.to_u32().to_le_bytes());
    }
    let mut out = ((b.len() + 4) as u32).to_le_bytes().to_vec();
    out.extend(b);
    out
}

// ---------- The connection ----------

pub struct Client {
    s: TcpStream,
}

impl Client {
    /// Connect to the server on this computer, with a name it shows.
    pub fn connect() -> Result<Client> {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], PORT));
        let s = TcpStream::connect_timeout(&addr, Duration::from_millis(800)).context("OpenRGB isn't running")?;
        s.set_read_timeout(Some(Duration::from_secs(5)))?;
        let mut c = Client { s };
        c.send(0, REQUEST_PROTOCOL_VERSION, &PROTOCOL.to_le_bytes())?;
        let _server = c.receive(REQUEST_PROTOCOL_VERSION)?;
        c.send(0, SET_CLIENT_NAME, b"Nyx Settings\0")?;
        Ok(c)
    }

    fn send(&mut self, device: u32, id: u32, body: &[u8]) -> Result<()> {
        let mut p = Vec::with_capacity(16 + body.len());
        p.extend(b"ORGB");
        p.extend(device.to_le_bytes());
        p.extend(id.to_le_bytes());
        p.extend((body.len() as u32).to_le_bytes());
        p.extend(body);
        self.s.write_all(&p).context("OpenRGB closed the connection")
    }

    fn receive(&mut self, want: u32) -> Result<Vec<u8>> {
        loop {
            let mut head = [0u8; 16];
            self.s.read_exact(&mut head).context("no answer from OpenRGB")?;
            if &head[0..4] != b"ORGB" {
                bail!("that isn't OpenRGB on port {PORT}");
            }
            let id = u32::from_le_bytes(head[8..12].try_into()?);
            let size = u32::from_le_bytes(head[12..16].try_into()?) as usize;
            let mut body = vec![0; size];
            self.s.read_exact(&mut body)?;
            // The server also sends notices (a device list changed); skip them.
            if id == want {
                return Ok(body);
            }
        }
    }

    pub fn devices(&mut self) -> Result<Vec<Device>> {
        self.send(0, REQUEST_CONTROLLER_COUNT, &[])?;
        let n = u32::from_le_bytes(self.receive(REQUEST_CONTROLLER_COUNT)?.get(0..4).context("bad count")?.try_into()?);
        let mut out = Vec::new();
        for i in 0..n {
            self.send(i, REQUEST_CONTROLLER_DATA, &PROTOCOL.to_le_bytes())?;
            let body = self.receive(REQUEST_CONTROLLER_DATA)?;
            out.push(parse_device(i, &body)?);
        }
        Ok(out)
    }

    /// Switch a device to one of its modes, with that mode's settings.
    pub fn set_mode(&mut self, device: u32, index: i32, m: &Mode) -> Result<()> {
        self.send(device, UPDATE_MODE, &mode_body(index, m))
    }

    /// Keep the mode on the device itself, so it stays after OpenRGB stops.
    pub fn save_mode(&mut self, device: u32, index: i32, m: &Mode) -> Result<()> {
        self.send(device, SAVE_MODE, &mode_body(index, m))
    }

    /// The device's "direct" (per-LED) mode, for setting colours yourself.
    pub fn custom_mode(&mut self, device: u32) -> Result<()> {
        self.send(device, SET_CUSTOM_MODE, &[])
    }

    pub fn set_leds(&mut self, device: u32, colors: &[Rgb]) -> Result<()> {
        self.send(device, UPDATE_LEDS, &colors_body(&[], colors))
    }

    pub fn set_zone(&mut self, device: u32, zone: u32, colors: &[Rgb]) -> Result<()> {
        self.send(device, UPDATE_ZONE_LEDS, &colors_body(&zone.to_le_bytes(), colors))
    }

    pub fn profiles(&mut self) -> Result<Vec<String>> {
        self.send(0, REQUEST_PROFILE_LIST, &[])?;
        let body = self.receive(REQUEST_PROFILE_LIST)?;
        let mut c = Cursor { b: &body, at: 0 };
        let _size = c.u32()?;
        let n = c.u16()?;
        (0..n).map(|_| c.string()).collect()
    }

    pub fn save_profile(&mut self, name: &str) -> Result<()> {
        let mut b = name.as_bytes().to_vec();
        b.push(0);
        self.send(0, REQUEST_SAVE_PROFILE, &b)
    }

    pub fn load_profile(&mut self, name: &str) -> Result<()> {
        let mut b = name.as_bytes().to_vec();
        b.push(0);
        self.send(0, REQUEST_LOAD_PROFILE, &b)
    }
}

/// Set a whole device to one colour: its static mode if it has one with a
/// colour, else per-LED colours.
pub fn set_colour(c: &mut Client, d: &Device, colour: Rgb) -> Result<()> {
    if let Some((i, m)) = d.modes.iter().enumerate().find(|(_, m)| {
        let n = m.name.to_lowercase();
        (n == "static" || n == "direct") && m.has(HAS_MODE_SPECIFIC_COLOR)
    }) {
        let mut m = m.clone();
        m.color_mode = COLOR_MODE_SPECIFIC;
        m.colors = vec![colour; m.colors_min.max(1) as usize];
        return c.set_mode(d.index, i as i32, &m);
    }
    c.custom_mode(d.index)?;
    c.set_leds(d.index, &vec![colour; d.leds.len().max(1)])
}

/// The OpenRGB server as a user service, so lighting works without a window.
pub const SERVICE: &str = "[Unit]\n\
Description=OpenRGB server (Settings)\n\
After=graphical-session.target\n\n\
[Service]\n\
ExecStart=/usr/bin/openrgb --server --server-port 6742 --noautoconnect\n\
Restart=on-failure\n\
RestartSec=5\n\n\
[Install]\n\
WantedBy=default.target\n";

/// Make sure the server runs: install and start its user service.
pub fn start_server() -> Result<()> {
    let dir = crate::sys::home().join(".config/systemd/user");
    std::fs::create_dir_all(&dir)?;
    let unit = dir.join("nyx-openrgb.service");
    if std::fs::read_to_string(&unit).ok().as_deref() != Some(SERVICE) {
        std::fs::write(&unit, SERVICE)?;
        crate::sys::run("systemctl", &["--user", "daemon-reload"])?;
    }
    crate::sys::run("systemctl", &["--user", "enable", "--now", "nyx-openrgb.service"])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A device as the server would send it (protocol 3).
    fn device_bytes() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(5i32.to_le_bytes());
        for s in ["Corsair Galleon 100 SD", "Corsair", "Keyboard", "1.0", "ABC", "HID: /dev/hidraw3"] {
            put_string(&mut b, s);
        }
        b.extend(2u16.to_le_bytes());
        b.extend(1i32.to_le_bytes());
        for (name, flags, cmode) in [("Direct", HAS_PER_LED_COLOR, COLOR_MODE_PER_LED), ("Static", HAS_MODE_SPECIFIC_COLOR | HAS_BRIGHTNESS, COLOR_MODE_SPECIFIC)] {
            let m = Mode {
                name: name.into(),
                value: 1,
                flags,
                brightness_max: 100,
                brightness: 80,
                colors_min: 1,
                colors_max: 1,
                color_mode: cmode,
                colors: if cmode == COLOR_MODE_SPECIFIC { vec![Rgb(1, 2, 3)] } else { vec![] },
                ..Default::default()
            };
            let body = mode_body(0, &m);
            // mode_body adds a size and an index; the device data has neither.
            b.extend(&body[8..]);
        }
        b.extend(1u16.to_le_bytes());
        put_string(&mut b, "Keyboard");
        b.extend(1i32.to_le_bytes());
        for v in [104u32, 104, 104] {
            b.extend(v.to_le_bytes());
        }
        b.extend(0u16.to_le_bytes());
        b.extend(2u16.to_le_bytes());
        for n in ["Key: A", "Key: B"] {
            put_string(&mut b, n);
            b.extend(0u32.to_le_bytes());
        }
        b.extend(2u16.to_le_bytes());
        b.extend(Rgb(255, 0, 0).to_u32().to_le_bytes());
        b.extend(Rgb(0, 0, 255).to_u32().to_le_bytes());
        let mut out = ((b.len() + 4) as u32).to_le_bytes().to_vec();
        out.extend(b);
        out
    }

    #[test]
    fn reads_a_device() {
        let d = parse_device(3, &device_bytes()).unwrap();
        assert_eq!(d.index, 3);
        assert_eq!(d.name, "Corsair Galleon 100 SD");
        assert_eq!(d.vendor, "Corsair");
        assert_eq!(d.kind_name(), "Keyboard");
        assert_eq!(d.modes.len(), 2);
        assert_eq!(d.mode().unwrap().name, "Static");
        assert_eq!(d.mode().unwrap().colors, vec![Rgb(1, 2, 3)]);
        assert!(d.mode().unwrap().has(HAS_BRIGHTNESS));
        assert_eq!(d.zones[0].leds_count, 104);
        assert_eq!(d.leds, vec!["Key: A", "Key: B"]);
        assert_eq!(d.colors, vec![Rgb(255, 0, 0), Rgb(0, 0, 255)]);
        assert_eq!(d.id(), "corsair-galleon-100-sd-abc-hid-dev-hidraw3");
    }

    #[test]
    fn short_data_is_an_error() {
        let b = device_bytes();
        assert!(parse_device(0, &b[..b.len() - 3]).is_err());
    }

    #[test]
    fn colours_round_trip() {
        let c = Rgb(0x12, 0x34, 0x56);
        assert_eq!(Rgb::from_u32(c.to_u32()), c);
        assert_eq!(c.hex(), "#123456");
        assert_eq!(Rgb::parse("#123456"), Some(c));
    }
}
