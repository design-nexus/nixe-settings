//! Drives, partitions and filesystems through UDisks2 on the system bus. UDisks
//! does the work (partitioning, formatting, resizing, checking, encryption,
//! fstab entries) and asks for permission through polkit itself, so nothing
//! here runs as root.
//!
//! [`load`] reads every object once into plain structs; [`watch`] calls back
//! when anything changes; the methods run asynchronously and return UDisks'
//! own error text.

use anyhow::{Result, anyhow};
use gtk::gio;
use gtk::glib::{self, Variant, VariantTy};
use gtk::prelude::*;
use std::collections::HashMap;

const BUS: &str = "org.freedesktop.UDisks2";
const ROOT: &str = "/org/freedesktop/UDisks2";
const MANAGER: &str = "/org/freedesktop/UDisks2/Manager";
const IF: &str = "org.freedesktop.UDisks2.";

/// Partition type GUIDs that mean "Windows".
const WINDOWS_TYPES: &[&str] = &[
    "e3c9e316-0b5c-4db8-817d-f92df00215ae", // Microsoft reserved
    "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7", // Microsoft basic data
    "de94bba4-06d1-4d40-a16a-bfd50179d6ac", // Windows recovery
];
const ESP: &str = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b";
/// Folders whose filesystem is the running system.
const SYSTEM_MOUNTS: &[&str] = &["/", "/boot", "/boot/efi", "/efi", "/home", "/usr", "/var"];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Health {
    /// Celsius.
    pub temperature: Option<f64>,
    pub power_on_hours: Option<u64>,
    /// NVMe critical warnings, or "failing" from SATA SMART.
    pub warnings: Vec<String>,
    pub supported: bool,
}

impl Health {
    pub fn ok(&self) -> bool {
        self.warnings.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Part {
    /// The block object (for Block/Partition/Filesystem/Encrypted calls).
    pub path: String,
    /// `/dev/nvme1n1p3`.
    pub device: String,
    pub number: u32,
    pub offset: u64,
    pub size: u64,
    /// The partition's name in a GPT table.
    pub name: String,
    pub ptype: String,
    pub flags: u64,
    /// `ntfs`, `ext4`, `crypto_LUKS`, `swap`, … ("" when unknown or empty).
    pub fs: String,
    pub usage: String,
    pub label: String,
    pub uuid: String,
    pub mounts: Vec<String>,
    /// Whether UDisks has a Filesystem interface on it (mountable).
    pub filesystem: bool,
    pub swap_active: bool,
    /// For LUKS: the unlocked device, with what's inside it.
    pub unlocked: Option<Box<Part>>,
    pub encrypted: bool,
    /// Holds the running system (directly or through what's unlocked in it).
    pub system: bool,
    /// Belongs to another operating system (Windows).
    pub other_os: bool,
    pub fstab: bool,
    /// A partition, rather than a filesystem on the whole drive.
    pub partition: bool,
}

impl Part {
    /// Mounted, active swap, or unlocked: it can't be changed until stopped.
    pub fn in_use(&self) -> bool {
        !self.mounts.is_empty() || self.swap_active || self.unlocked.is_some()
    }

    /// The name to show: its label, else its partition name, else the device.
    pub fn title(&self) -> String {
        if !self.label.is_empty() {
            self.label.clone()
        } else if !self.name.is_empty() {
            self.name.clone()
        } else {
            self.short()
        }
    }

    /// `nvme1n1p3`.
    pub fn short(&self) -> String {
        self.device.trim_start_matches("/dev/").to_string()
    }

    /// What's on it, in words: "NTFS", "Encrypted (btrfs inside)", "Empty".
    pub fn kind(&self) -> String {
        if self.encrypted {
            return match &self.unlocked {
                Some(inner) if !inner.fs.is_empty() => format!("Encrypted · {} inside", fs_name(&inner.fs)),
                Some(_) => "Encrypted, unlocked".into(),
                None => "Encrypted, locked".into(),
            };
        }
        if self.fs.is_empty() { "Empty or unknown".into() } else { fs_name(&self.fs) }
    }

    /// The filesystem that actually holds files (the inside of an unlocked
    /// encrypted partition).
    pub fn content(&self) -> &Part {
        self.unlocked.as_deref().unwrap_or(self)
    }
}

/// A run of the drive: a partition or free space.
#[derive(Debug, Clone, PartialEq)]
pub enum Segment {
    Part(usize),
    Free { offset: u64, size: u64 },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Drive {
    pub path: String,
    /// The whole-drive block device object, and its `/dev` name.
    pub block: String,
    pub device: String,
    pub model: String,
    pub size: u64,
    pub removable: bool,
    pub ejectable: bool,
    pub can_power_off: bool,
    /// "nvme", "usb", "sata", "sdio", "loop".
    pub bus: String,
    pub rotational: bool,
    /// "gpt", "dos", or "" with no partition table.
    pub table: String,
    pub parts: Vec<Part>,
    pub health: Health,
    pub nvme: bool,
    /// For a loop device: the image file.
    pub backing_file: String,
}

impl Drive {
    pub fn system(&self) -> bool {
        self.parts.iter().any(|p| p.system)
    }
    pub fn other_os(&self) -> bool {
        self.parts.iter().any(|p| p.other_os)
    }
    pub fn in_use(&self) -> bool {
        self.parts.iter().any(|p| p.in_use())
    }

    pub fn title(&self) -> String {
        if !self.backing_file.is_empty() {
            let file =
                std::path::Path::new(&self.backing_file).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
            return format!("Disk image {file}");
        }
        if self.model.is_empty() { self.device.trim_start_matches("/dev/").to_string() } else { self.model.clone() }
    }

    /// Partitions and the free space between them, in order. Gaps under 4 MiB
    /// (alignment) aren't worth showing.
    pub fn segments(&self) -> Vec<Segment> {
        const MIN_FREE: u64 = 4 << 20;
        const START: u64 = 1 << 20;
        let mut out = Vec::new();
        if self.table.is_empty() {
            if self.parts.is_empty() {
                out.push(Segment::Free { offset: 0, size: self.size });
            } else {
                out.push(Segment::Part(0));
            }
            return out;
        }
        let mut order: Vec<usize> = (0..self.parts.len()).collect();
        order.sort_by_key(|&i| self.parts[i].offset);
        let mut at = START;
        // GPT keeps a backup table in the last 33 sectors.
        let end = self.size.saturating_sub(if self.table == "gpt" { 1 << 20 } else { 0 });
        for i in order {
            let p = &self.parts[i];
            if p.offset > at && p.offset - at >= MIN_FREE {
                out.push(Segment::Free { offset: at, size: p.offset - at });
            }
            out.push(Segment::Part(i));
            at = at.max(p.offset + p.size);
        }
        if end > at && end - at >= MIN_FREE {
            out.push(Segment::Free { offset: at, size: end - at });
        }
        out
    }

    /// Free space right after partition `i`, which it could grow into.
    pub fn free_after(&self, i: usize) -> u64 {
        let segs = self.segments();
        segs.iter()
            .position(|s| *s == Segment::Part(i))
            .and_then(|k| segs.get(k + 1))
            .map(|s| if let Segment::Free { size, .. } = s { *size } else { 0 })
            .unwrap_or(0)
    }
}

pub fn fs_name(fs: &str) -> String {
    match fs {
        "vfat" => "FAT32".into(),
        "exfat" => "exFAT".into(),
        "ntfs" => "NTFS".into(),
        "ext4" | "ext3" | "ext2" => fs.to_uppercase().replace("EXT", "ext"),
        "crypto_LUKS" => "LUKS".into(),
        "swap" => "Swap".into(),
        other => other.to_string(),
    }
}

// ---------- Reading ----------

fn bus() -> Result<gio::DBusConnection> {
    gio::bus_get_sync(gio::BusType::System, None::<&gio::Cancellable>).map_err(|e| anyhow!("{e}"))
}

type Props = HashMap<String, Variant>;
/// Object path → interface (without the UDisks2 prefix) → properties.
type Objects = HashMap<String, HashMap<String, Props>>;

fn managed() -> Result<Objects> {
    let reply = bus()?
        .call_sync(
            Some(BUS),
            ROOT,
            "org.freedesktop.DBus.ObjectManager",
            "GetManagedObjects",
            None,
            Some(VariantTy::new("(a{oa{sa{sv}}})").unwrap()),
            gio::DBusCallFlags::NONE,
            5000,
            None::<&gio::Cancellable>,
        )
        .map_err(|e| anyhow!("{e}"))?;
    let mut out = Objects::new();
    let dict = reply.child_value(0);
    for entry in dict.iter() {
        let path = entry.child_value(0).str().unwrap_or_default().to_string();
        let mut ifaces = HashMap::new();
        for iface in entry.child_value(1).iter() {
            let name = iface.child_value(0).str().unwrap_or_default().trim_start_matches(IF).to_string();
            let mut props = Props::new();
            for p in iface.child_value(1).iter() {
                let k = p.child_value(0).str().unwrap_or_default().to_string();
                let v = p.child_value(1).as_variant().unwrap_or_else(|| p.child_value(1));
                props.insert(k, v);
            }
            ifaces.insert(name, props);
        }
        out.insert(path, ifaces);
    }
    Ok(out)
}

fn s(p: &Props, k: &str) -> String {
    p.get(k).and_then(|v| v.str().map(String::from)).unwrap_or_default()
}
fn t(p: &Props, k: &str) -> u64 {
    p.get(k).and_then(|v| v.get::<u64>().or_else(|| v.get::<u32>().map(u64::from))).unwrap_or(0)
}
fn b(p: &Props, k: &str) -> bool {
    p.get(k).and_then(|v| v.get::<bool>()).unwrap_or(false)
}
fn o(p: &Props, k: &str) -> String {
    p.get(k).and_then(|v| v.str().map(String::from)).filter(|x| x != "/").unwrap_or_default()
}
/// A NUL-terminated byte string (`ay`): device names and mount points.
fn bytes(v: &Variant) -> String {
    let raw: Vec<u8> = v.get::<Vec<u8>>().unwrap_or_default();
    String::from_utf8_lossy(raw.strip_suffix(&[0]).unwrap_or(&raw)).to_string()
}
fn mounts(p: &Props) -> Vec<String> {
    p.get("MountPoints").map(|v| v.iter().map(|m| bytes(&m)).collect()).unwrap_or_default()
}

/// Every drive with its partitions, and loop devices with an image behind them.
pub fn load() -> Result<Vec<Drive>> {
    Ok(build(&managed()?))
}

fn part_of(path: &str, objs: &Objects) -> Part {
    let ifs = &objs[path];
    let empty = Props::new();
    let block = ifs.get("Block").unwrap_or(&empty);
    let part = ifs.get("Partition");
    let fsys = ifs.get("Filesystem");
    let mut p = Part {
        path: path.to_string(),
        device: block.get("PreferredDevice").or(block.get("Device")).map(bytes).unwrap_or_default(),
        fs: s(block, "IdType"),
        usage: s(block, "IdUsage"),
        label: s(block, "IdLabel"),
        uuid: s(block, "IdUUID"),
        size: t(block, "Size"),
        filesystem: fsys.is_some(),
        mounts: fsys.map(mounts).unwrap_or_default(),
        swap_active: ifs.get("Swapspace").is_some_and(|sw| b(sw, "Active")),
        encrypted: ifs.contains_key("Encrypted"),
        fstab: block.get("Configuration").is_some_and(|c| c.iter().any(|item| item.child_value(0).str() == Some("fstab"))),
        ..Default::default()
    };
    if let Some(pt) = part {
        p.partition = true;
        p.number = t(pt, "Number") as u32;
        p.offset = t(pt, "Offset");
        p.size = t(pt, "Size");
        p.name = s(pt, "Name");
        p.ptype = s(pt, "Type").to_lowercase();
        p.flags = t(pt, "Flags");
    }
    if p.encrypted {
        let clear = ifs.get("Encrypted").map(|e| o(e, "CleartextDevice")).unwrap_or_default();
        if objs.contains_key(&clear) {
            p.unlocked = Some(Box::new(part_of(&clear, objs)));
        }
    }
    let content = p.content();
    p.system = content.mounts.iter().any(|m| SYSTEM_MOUNTS.contains(&m.as_str()))
        || p.mounts.iter().any(|m| SYSTEM_MOUNTS.contains(&m.as_str()))
        || b(block, "HintSystem") && p.ptype == ESP && p.mounts.iter().any(|m| m.starts_with("/boot") || m == "/efi");
    p.other_os = p.fs == "ntfs" || WINDOWS_TYPES.contains(&p.ptype.as_str()) || p.label.eq_ignore_ascii_case("windows");
    p
}

fn build(objs: &Objects) -> Vec<Drive> {
    let mut drives: Vec<Drive> = Vec::new();
    // Whole-drive blocks: they belong to a drive (or are a loop device) and
    // aren't partitions or unlocked insides.
    let mut paths: Vec<&String> = objs.keys().filter(|p| p.starts_with("/org/freedesktop/UDisks2/block_devices/")).collect();
    paths.sort();
    for path in paths {
        let ifs = &objs[path];
        let Some(block) = ifs.get("Block") else { continue };
        if ifs.contains_key("Partition") || !o(block, "CryptoBackingDevice").is_empty() || b(block, "HintIgnore") {
            continue;
        }
        let device = block.get("Device").map(bytes).unwrap_or_default();
        if device.starts_with("/dev/zram") || device.starts_with("/dev/dm-") || t(block, "Size") == 0 {
            continue;
        }
        let drive_path = o(block, "Drive");
        let loop_file = ifs.get("Loop").map(|l| l.get("BackingFile").map(bytes).unwrap_or_default()).unwrap_or_default();
        if drive_path.is_empty() && loop_file.is_empty() {
            continue;
        }
        let empty = Props::new();
        let difs = objs.get(&drive_path);
        let drive = difs.and_then(|d| d.get("Drive")).unwrap_or(&empty);
        let nvme_if = difs.and_then(|d| d.get("NVMe.Controller"));
        let ata = difs.and_then(|d| d.get("Drive.Ata"));
        let mut d = Drive {
            path: drive_path.clone(),
            block: path.clone(),
            device: device.clone(),
            model: s(drive, "Model").trim().to_string(),
            size: t(block, "Size"),
            removable: b(drive, "Removable") || b(drive, "MediaRemovable"),
            ejectable: b(drive, "Ejectable"),
            can_power_off: b(drive, "CanPowerOff"),
            bus: if !loop_file.is_empty() {
                "loop".into()
            } else if nvme_if.is_some() || device.contains("nvme") {
                "nvme".into()
            } else if !s(drive, "ConnectionBus").is_empty() {
                s(drive, "ConnectionBus")
            } else {
                "sata".into()
            },
            rotational: drive.get("RotationRate").and_then(|v| v.get::<i32>()).unwrap_or(0) != 0,
            table: ifs.get("PartitionTable").map(|t| s(t, "Type")).unwrap_or_default(),
            nvme: nvme_if.is_some(),
            backing_file: loop_file,
            ..Default::default()
        };
        if let Some(n) = nvme_if {
            d.health.supported = true;
            d.health.temperature =
                n.get("SmartTemperature").and_then(|v| v.get::<u16>()).filter(|k| *k > 0).map(|k| k as f64 - 273.15);
            d.health.power_on_hours = Some(t(n, "SmartPowerOnHours")).filter(|h| *h > 0);
            d.health.warnings = n.get("SmartCriticalWarning").and_then(|v| v.get::<Vec<String>>()).unwrap_or_default();
        } else if let Some(a) = ata.filter(|a| b(a, "SmartSupported")) {
            d.health.supported = true;
            d.health.temperature =
                a.get("SmartTemperature").and_then(|v| v.get::<f64>()).filter(|k| *k > 0.0).map(|k| k - 273.15);
            d.health.power_on_hours = Some(t(a, "SmartPowerOnSeconds") / 3600).filter(|h| *h > 0);
            if b(a, "SmartFailing") {
                d.health.warnings.push("failing".into());
            }
        }
        if d.table.is_empty() {
            // A filesystem on the whole drive (a USB stick formatted as is).
            let whole = part_of(path, objs);
            if !whole.fs.is_empty() {
                d.parts.push(whole);
            }
        } else {
            let mut parts: Vec<Part> = objs
                .iter()
                .filter(|(_, ifs)| ifs.get("Partition").is_some_and(|p| o(p, "Table") == *path))
                .map(|(p, _)| part_of(p, objs))
                .collect();
            parts.sort_by_key(|p| p.offset);
            d.parts = parts;
        }
        // An EFI partition next to Windows partitions is Windows' too.
        if d.parts.iter().any(|p| p.fs == "ntfs" || WINDOWS_TYPES.contains(&p.ptype.as_str())) {
            for p in d.parts.iter_mut().filter(|p| p.ptype == ESP && !p.system) {
                p.other_os = true;
            }
        }
        drives.push(d);
    }
    // The system drive first, then by name.
    drives.sort_by_key(|d| (!d.system(), d.removable, d.device.clone()));
    drives
}

/// How much of a mounted filesystem is used, from the folder it's mounted at.
pub fn used(mount: &str) -> Option<(u64, u64)> {
    let info =
        gio::File::for_path(mount).query_filesystem_info("filesystem::size,filesystem::used", None::<&gio::Cancellable>).ok()?;
    Some((info.attribute_uint64("filesystem::used"), info.attribute_uint64("filesystem::size")))
}

/// Call `f` (soon, once) whenever anything UDisks knows about changes.
pub fn watch(owner: &impl IsA<gtk::Widget>, f: impl Fn() + 'static) {
    let Ok(conn) = bus() else { return };
    let pending: std::rc::Rc<std::cell::Cell<Option<glib::SourceId>>> = Default::default();
    let weak = owner.upcast_ref::<gtk::Widget>().downgrade();
    let f = std::rc::Rc::new(f);
    #[allow(deprecated)]
    let id = conn.signal_subscribe(Some(BUS), None, None, None, None, gio::DBusSignalFlags::NONE, move |_, _, _, _, _, _| {
        if weak.upgrade().is_none() {
            return;
        }
        if let Some(id) = pending.take() {
            id.remove();
        }
        let (f, p) = (f.clone(), pending.clone());
        pending.set(Some(glib::timeout_add_local_once(std::time::Duration::from_millis(400), move || {
            p.set(None);
            f();
        })));
    });
    // Stop listening once the page is gone.
    let conn2 = conn.clone();
    let id = std::cell::Cell::new(Some(id));
    owner.upcast_ref::<gtk::Widget>().connect_destroy(move |_| {
        if let Some(id) = id.take() {
            #[allow(deprecated)]
            conn2.signal_unsubscribe(id);
        }
    });
}

// ---------- What UDisks can do ----------

/// Resize support for a filesystem type.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ResizeModes {
    pub offline_shrink: bool,
    pub offline_grow: bool,
    pub online_shrink: bool,
    pub online_grow: bool,
}

/// Whether UDisks can resize `fs`, how, and the program missing if it can't.
pub fn can_resize(fs: &str) -> (Option<ResizeModes>, String) {
    let Ok(conn) = bus() else { return (None, String::new()) };
    let Ok(r) = conn.call_sync(
        Some(BUS),
        MANAGER,
        "org.freedesktop.UDisks2.Manager",
        "CanResize",
        Some(&(fs,).to_variant()),
        Some(VariantTy::new("(bts)").unwrap()),
        gio::DBusCallFlags::NONE,
        3000,
        None::<&gio::Cancellable>,
    ) else {
        return (None, String::new());
    };
    let (ok, mode, missing): (bool, u64, String) = r.get().unwrap_or((false, 0, String::new()));
    if !ok {
        return (None, missing);
    }
    (Some(modes(mode)), String::new())
}

/// libblockdev's resize flags.
pub fn modes(m: u64) -> ResizeModes {
    ResizeModes { offline_shrink: m & 2 != 0, offline_grow: m & 4 != 0, online_shrink: m & 8 != 0, online_grow: m & 16 != 0 }
}

/// Whether UDisks can make `fs`, and the program missing if not.
pub fn can_format(fs: &str) -> (bool, String) {
    manager_bs("CanFormat", fs)
}

pub fn can_check(fs: &str) -> (bool, String) {
    manager_bs("CanCheck", fs)
}

fn manager_bs(method: &str, fs: &str) -> (bool, String) {
    let Ok(conn) = bus() else { return (false, String::new()) };
    conn.call_sync(
        Some(BUS),
        MANAGER,
        "org.freedesktop.UDisks2.Manager",
        method,
        Some(&(fs,).to_variant()),
        Some(VariantTy::new("(bs)").unwrap()),
        gio::DBusCallFlags::NONE,
        3000,
        None::<&gio::Cancellable>,
    )
    .ok()
    .and_then(|r| r.get::<(bool, String)>())
    .unwrap_or((false, String::new()))
}

/// The package that has a missing program.
pub fn package_for(program: &str) -> Option<&'static str> {
    Some(match program {
        p if p.contains("ntfs") => "ntfs-3g",
        p if p.contains("xfs") => "xfsprogs",
        p if p.contains("exfat") => "exfatprogs",
        p if p.contains("fatresize") => "fatresize",
        p if p.contains("btrfs") => "btrfs-progs",
        p if p.contains("dosfs") || p.contains("fsck.vfat") || p.contains("mkfs.vfat") => "dosfstools",
        p if p.contains("e2fs") || p.contains("resize2fs") || p.contains("mkfs.ext") => "e2fsprogs",
        _ => return None,
    })
}

/// How small partition `p` may become: what's used plus a margin, or its whole
/// size when that isn't known (not mounted, so nothing to measure).
pub fn smallest(p: &Part, used: Option<u64>) -> u64 {
    match used {
        Some(u) => (u + u / 10 + (256 << 20)).min(p.size),
        None => p.size,
    }
}

/// Round down to whole MiB, as partitions are aligned.
pub fn align(bytes: u64) -> u64 {
    bytes / (1 << 20) * (1 << 20)
}

// ---------- Changing things ----------

type Opts = HashMap<String, Variant>;

fn opts(items: &[(&str, Variant)]) -> Opts {
    items.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

/// One UDisks method call, letting polkit ask for permission. Errors come back
/// as UDisks words them, without the D-Bus prefix.
async fn call(path: &str, iface: &str, method: &str, args: Variant, reply: &str) -> Result<Variant> {
    let conn = bus()?;
    let r = conn
        .call_future(
            Some(BUS),
            path,
            &format!("{IF}{iface}"),
            method,
            Some(&args),
            Some(VariantTy::new(reply).unwrap()),
            gio::DBusCallFlags::ALLOW_INTERACTIVE_AUTHORIZATION,
            i32::MAX,
        )
        .await;
    r.map_err(|mut e| {
        gio::DBusError::strip_remote_error(&mut e);
        let msg = e.message().to_string();
        // Polkit's refusal, said plainly.
        if msg.contains("Not authorized") || msg.contains("not authorized") {
            anyhow!("Permission wasn't given.")
        } else {
            anyhow!("{}", msg.trim())
        }
    })
}

/// How to fill a filesystem being made.
#[derive(Debug, Clone, Default)]
pub struct Make {
    /// `ext4`, `btrfs`, `vfat`, `exfat`, `ntfs`, or `empty`.
    pub fs: String,
    pub label: String,
    /// LUKS2 with this passphrase.
    pub passphrase: Option<String>,
    /// Overwrite the old contents with zeros (slow).
    pub erase: bool,
}

fn format_opts(m: &Make) -> Opts {
    let mut o = opts(&[("take-ownership", true.to_variant()), ("update-partition-type", true.to_variant())]);
    if !m.label.is_empty() {
        o.insert("label".into(), m.label.to_variant());
    }
    if let Some(p) = &m.passphrase {
        o.insert("encrypt.passphrase".into(), p.to_variant());
        o.insert("encrypt.type".into(), "luks2".to_variant());
    }
    if m.erase {
        o.insert("erase".into(), "zero".to_variant());
    }
    // Close anything open on it first (an unlocked LUKS inside).
    o.insert("tear-down".into(), true.to_variant());
    o
}

/// Make a filesystem on a partition or a whole drive (erasing it).
pub async fn format(block: &str, m: &Make) -> Result<()> {
    call(block, "Block", "Format", (m.fs.as_str(), format_opts(m)).to_variant(), "()").await.map(|_| ())
}

/// A new, empty partition table on a whole drive ("gpt" or "dos").
pub async fn new_table(block: &str, kind: &str) -> Result<()> {
    let o = opts(&[("tear-down", true.to_variant())]);
    call(block, "Block", "Format", (kind, o).to_variant(), "()").await.map(|_| ())
}

/// A new partition in free space, with a filesystem made on it.
pub async fn create(table_block: &str, offset: u64, size: u64, name: &str, m: &Make) -> Result<String> {
    let args = (offset, size, "", name, Opts::new(), m.fs.as_str(), format_opts(m)).to_variant();
    let r = call(table_block, "PartitionTable", "CreatePartitionAndFormat", args, "(o)").await?;
    Ok(r.child_value(0).str().unwrap_or_default().to_string())
}

pub async fn delete(part: &str) -> Result<()> {
    call(part, "Partition", "Delete", (opts(&[("tear-down", true.to_variant())]),).to_variant(), "()").await.map(|_| ())
}

/// Resize partition `p` (and its filesystem) to `size` bytes, in the safe
/// order: growing moves the partition's end first, shrinking the filesystem's.
pub async fn resize(p: &Part, size: u64) -> Result<()> {
    let fs_path = p.content().path.clone();
    let has_fs = p.content().filesystem;
    // resize2fs only resizes an unmounted ext filesystem that's just been
    // checked in full.
    if has_fs && p.content().fs.starts_with("ext") && p.content().mounts.is_empty() {
        call(&fs_path, "Filesystem", "Repair", (Opts::new(),).to_variant(), "(b)").await?;
    }
    if size > p.size {
        call(&p.path, "Partition", "Resize", (size, Opts::new()).to_variant(), "()").await?;
        if p.unlocked.is_some() {
            call(&fs_path, "Block", "Resize", (0u64, Opts::new()).to_variant(), "()").await.ok();
        }
        if has_fs {
            call(&fs_path, "Filesystem", "Resize", (0u64, Opts::new()).to_variant(), "()").await?;
        }
    } else {
        if has_fs {
            // Leave the filesystem a little short of the partition (LUKS header).
            let fs_size = if p.encrypted { size.saturating_sub(32 << 20) } else { size };
            call(&fs_path, "Filesystem", "Resize", (fs_size, Opts::new()).to_variant(), "()").await?;
        }
        call(&p.path, "Partition", "Resize", (size, Opts::new()).to_variant(), "()").await?;
    }
    Ok(())
}

pub async fn set_label(fs_block: &str, label: &str) -> Result<()> {
    call(fs_block, "Filesystem", "SetLabel", (label, Opts::new()).to_variant(), "()").await.map(|_| ())
}

pub async fn mount(fs_block: &str) -> Result<String> {
    let r = call(fs_block, "Filesystem", "Mount", (Opts::new(),).to_variant(), "(s)").await?;
    Ok(r.child_value(0).str().unwrap_or_default().to_string())
}

pub async fn unmount(fs_block: &str) -> Result<()> {
    call(fs_block, "Filesystem", "Unmount", (Opts::new(),).to_variant(), "()").await.map(|_| ())
}

/// Check (and with `repair`, fix) an unmounted filesystem. True when it's fine.
pub async fn check(fs_block: &str, repair: bool) -> Result<bool> {
    let method = if repair { "Repair" } else { "Check" };
    let r = call(fs_block, "Filesystem", method, (Opts::new(),).to_variant(), "(b)").await?;
    Ok(r.child_value(0).get::<bool>().unwrap_or(false))
}

pub async fn unlock(part: &str, passphrase: &str) -> Result<()> {
    call(part, "Encrypted", "Unlock", (passphrase, Opts::new()).to_variant(), "(o)").await.map(|_| ())
}

pub async fn lock(part: &str) -> Result<()> {
    call(part, "Encrypted", "Lock", (Opts::new(),).to_variant(), "()").await.map(|_| ())
}

pub async fn change_passphrase(part: &str, old: &str, new: &str) -> Result<()> {
    call(part, "Encrypted", "ChangePassphrase", (old, new, Opts::new()).to_variant(), "()").await.map(|_| ())
}

pub async fn power_off(drive: &str) -> Result<()> {
    call(drive, "Drive", "PowerOff", (Opts::new(),).to_variant(), "()").await.map(|_| ())
}

pub async fn eject(drive: &str) -> Result<()> {
    call(drive, "Drive", "Eject", (Opts::new(),).to_variant(), "()").await.map(|_| ())
}

/// NVMe wear and the rest of its health log.
pub async fn nvme_details(drive: &str) -> Result<HashMap<String, Variant>> {
    let r = call(drive, "NVMe.Controller", "SmartGetAttributes", (Opts::new(),).to_variant(), "(a{sv})").await?;
    Ok(r.child_value(0).get::<HashMap<String, Variant>>().unwrap_or_default())
}

/// A startup mount: an fstab entry for `fs_block` at `dir` with `options`.
pub async fn add_fstab(fs_block: &str, dir: &str, fs: &str, options: &str) -> Result<()> {
    let uuid_name = format!("UUID={}", block_uuid(fs_block).unwrap_or_default());
    let mut item: HashMap<String, Variant> = HashMap::new();
    item.insert("fsname".into(), Variant::array_from_fixed_array(&nul(&uuid_name)));
    item.insert("dir".into(), Variant::array_from_fixed_array(&nul(dir)));
    item.insert("type".into(), Variant::array_from_fixed_array(&nul(fs)));
    item.insert("opts".into(), Variant::array_from_fixed_array(&nul(options)));
    item.insert("freq".into(), 0i32.to_variant());
    item.insert("passno".into(), (if fs == "ext4" { 2i32 } else { 0i32 }).to_variant());
    let entry = ("fstab", item).to_variant();
    let args = Variant::tuple_from_iter([entry, Opts::new().to_variant()]);
    call(fs_block, "Block", "AddConfigurationItem", args, "()").await.map(|_| ())
}

/// Remove the fstab entries UDisks lists for `fs_block`.
pub async fn remove_fstab(fs_block: &str) -> Result<()> {
    let objs = managed()?;
    let items: Vec<Variant> = objs
        .get(fs_block)
        .and_then(|i| i.get("Block"))
        .and_then(|b| b.get("Configuration"))
        .map(|c| c.iter().filter(|item| item.child_value(0).str() == Some("fstab")).collect())
        .unwrap_or_default();
    for item in items {
        let args = Variant::tuple_from_iter([item, Opts::new().to_variant()]);
        call(fs_block, "Block", "RemoveConfigurationItem", args, "()").await?;
    }
    Ok(())
}

fn block_uuid(fs_block: &str) -> Option<String> {
    let objs = managed().ok()?;
    Some(s(objs.get(fs_block)?.get("Block")?, "IdUUID")).filter(|u| !u.is_empty())
}

fn nul(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

/// Running UDisks jobs on `object`: (what, fraction done if known).
pub fn jobs_for(object: &str) -> Vec<(String, Option<f64>)> {
    let Ok(objs) = managed() else { return Vec::new() };
    objs.values()
        .filter_map(|ifs| ifs.get("Job"))
        .filter(|j| j.get("Objects").is_some_and(|o| o.iter().any(|x| x.str() == Some(object))))
        .map(|j| {
            let progress = b(j, "ProgressValid").then(|| j.get("Progress").and_then(|v| v.get::<f64>()).unwrap_or(0.0));
            (s(j, "Operation"), progress)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(offset: u64, size: u64) -> Part {
        Part { offset, size, partition: true, ..Default::default() }
    }

    const G: u64 = 1 << 30;

    #[test]
    fn finds_free_space_between_partitions() {
        let d = Drive {
            size: 100 * G,
            table: "gpt".into(),
            parts: vec![part(1 << 20, 10 * G), part(30 * G, 20 * G)],
            ..Default::default()
        };
        let segs = d.segments();
        assert_eq!(segs.len(), 4);
        assert_eq!(segs[0], Segment::Part(0));
        assert!(matches!(segs[1], Segment::Free { offset, size } if offset == (1 << 20) + 10 * G && size == 20 * G - (1 << 20)));
        assert_eq!(segs[2], Segment::Part(1));
        assert!(matches!(segs[3], Segment::Free { size, .. } if size == 100 * G - 50 * G - (1 << 20)));
        assert_eq!(d.free_after(0), 20 * G - (1 << 20));
        assert_eq!(d.free_after(1), 100 * G - 50 * G - (1 << 20));
    }

    #[test]
    fn a_drive_without_a_table_is_free_or_one_filesystem() {
        let d = Drive { size: 8 * G, ..Default::default() };
        assert_eq!(d.segments(), vec![Segment::Free { offset: 0, size: 8 * G }]);
        let d = Drive {
            size: 8 * G,
            parts: vec![Part { fs: "vfat".into(), size: 8 * G, ..Default::default() }],
            ..Default::default()
        };
        assert_eq!(d.segments(), vec![Segment::Part(0)]);
    }

    #[test]
    fn small_gaps_are_alignment_not_free_space() {
        let d = Drive {
            size: 10 * G,
            table: "gpt".into(),
            parts: vec![part(1 << 20, 5 * G), part(5 * G + (2 << 20), 5 * G - (4 << 20))],
            ..Default::default()
        };
        assert!(d.segments().iter().all(|s| matches!(s, Segment::Part(_))));
    }

    #[test]
    fn in_use_and_titles() {
        let mut p = Part { device: "/dev/sdb1".into(), ..Default::default() };
        assert!(!p.in_use());
        assert_eq!(p.title(), "sdb1");
        p.mounts.push("/mnt/x".into());
        assert!(p.in_use());
        p.label = "Photos".into();
        assert_eq!(p.title(), "Photos");
        let locked = Part { encrypted: true, ..Default::default() };
        assert_eq!(locked.kind(), "Encrypted, locked");
        let open = Part {
            encrypted: true,
            unlocked: Some(Box::new(Part { fs: "btrfs".into(), ..Default::default() })),
            ..Default::default()
        };
        assert!(open.in_use());
        assert_eq!(open.kind(), "Encrypted · btrfs inside");
    }

    #[test]
    fn resize_flags_and_limits() {
        let m = modes(22);
        assert!(m.offline_shrink && m.offline_grow && m.online_grow && !m.online_shrink);
        let m = modes(24);
        assert!(m.online_shrink && m.online_grow && !m.offline_grow);
        let p = Part { size: 100 * G, ..Default::default() };
        assert_eq!(smallest(&p, None), 100 * G);
        let s = smallest(&p, Some(10 * G));
        assert!(s > 10 * G && s < 12 * G);
        assert_eq!(align(G + 12345), G);
    }

    #[test]
    fn packages_for_missing_programs() {
        assert_eq!(package_for("ntfsresize"), Some("ntfs-3g"));
        assert_eq!(package_for("mkfs.ntfs"), Some("ntfs-3g"));
        assert_eq!(package_for("mkfs.xfs"), Some("xfsprogs"));
        assert_eq!(package_for("frobnicate"), None);
    }

    /// Prints what UDisks reports here: `cargo test -p nixe-settings drives_here -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn drives_here() {
        for d in load().unwrap() {
            println!(
                "{} {} {} table={} system={} other_os={} health={:?}",
                d.device,
                d.title(),
                d.bus,
                d.table,
                d.system(),
                d.other_os(),
                d.health
            );
            for p in &d.parts {
                println!(
                    "  {} {:>6} GiB {} '{}' kind={} mounts={:?} system={} other_os={} in_use={} fstab={}",
                    p.short(),
                    p.size >> 30,
                    p.fs,
                    p.title(),
                    p.kind(),
                    p.content().mounts,
                    p.system,
                    p.other_os,
                    p.in_use(),
                    p.fstab
                );
            }
            for s in d.segments() {
                if let Segment::Free { size, .. } = s {
                    println!("  free {} MiB", size >> 20);
                }
            }
        }
    }

    /// Runs every change on a throwaway disk image, never a real drive:
    /// `NIXE_LOOP_TEST=/path/to/image cargo test -p nixe-settings loop_device_operations -- --ignored --nocapture`
    /// with the image attached by `udisksctl loop-setup -f`.
    #[test]
    #[ignore]
    fn loop_device_operations() {
        let Ok(image) = std::env::var("NIXE_LOOP_TEST") else { return };
        let ctx = glib::MainContext::default();
        let drive = || load().unwrap().into_iter().find(|d| d.backing_file == image).expect("the image is attached");
        let d = drive();
        assert!(!d.system() && d.bus == "loop", "only ever a loop device: {d:?}");
        let step = |what: &str| println!("== {what}");
        // A desktop automounter may mount what's just been made.
        let unmounted = |i: usize| {
            let p = drive().parts[i].content().clone();
            if !p.mounts.is_empty() {
                ctx.block_on(unmount(&p.path)).unwrap();
            }
        };

        step("new GPT table");
        ctx.block_on(new_table(&d.block, "gpt")).unwrap();
        let d = drive();
        assert_eq!(d.table, "gpt");
        assert!(d.parts.is_empty());

        step("new ext4 partition, 600 MiB, named DATA");
        let m = Make { fs: "ext4".into(), label: "DATA".into(), ..Default::default() };
        ctx.block_on(create(&d.block, 1 << 20, 600 << 20, "Data", &m)).unwrap();
        let d = drive();
        assert_eq!(d.parts.len(), 1);
        let p = d.parts[0].clone();
        assert_eq!((p.fs.as_str(), p.label.as_str(), p.name.as_str()), ("ext4", "DATA", "Data"));

        step("rename to Test");
        ctx.block_on(set_label(&p.path, "Test")).unwrap();
        assert_eq!(drive().parts[0].label, "Test");

        step("check");
        unmounted(0);
        assert!(ctx.block_on(check(&p.path, false)).unwrap());

        step("mount, measure, unmount");
        unmounted(0);
        let at = ctx.block_on(mount(&p.path)).unwrap();
        let (used_now, size) = used(&at).unwrap();
        println!("   mounted at {at}: {used_now} of {size}");
        ctx.block_on(unmount(&p.path)).unwrap();

        step("grow to 900 MiB");
        let p = drive().parts[0].clone();
        ctx.block_on(resize(&p, 900 << 20)).unwrap();
        assert_eq!(drive().parts[0].size, 900 << 20);

        step("shrink to 500 MiB");
        unmounted(0);
        let p = drive().parts[0].clone();
        ctx.block_on(resize(&p, 500 << 20)).unwrap();
        assert_eq!(drive().parts[0].size, 500 << 20);
        unmounted(0);
        assert!(ctx.block_on(check(&p.path, false)).unwrap(), "still consistent after shrinking");

        step("exFAT partition in the free space after it");
        let d = drive();
        let free = d
            .segments()
            .into_iter()
            .find_map(|s| if let Segment::Free { offset, size } = s { Some((offset, size)) } else { None })
            .unwrap();
        let m = Make { fs: "exfat".into(), label: "SHARE".into(), ..Default::default() };
        ctx.block_on(create(&d.block, free.0, 400 << 20, "Share", &m)).unwrap();
        let d = drive();
        assert_eq!(d.parts.len(), 2);
        assert_eq!(d.parts[1].fs, "exfat");

        step("format the first as encrypted btrfs");
        unmounted(0);
        let p = d.parts[0].clone();
        let m =
            Make { fs: "btrfs".into(), label: "Secret".into(), passphrase: Some("correct horse 1".into()), ..Default::default() };
        ctx.block_on(format(&p.path, &m)).unwrap();
        let p = drive().parts[0].clone();
        assert!(p.encrypted, "{p:?}");
        println!("   {} ({})", p.kind(), if p.unlocked.is_some() { "unlocked" } else { "locked" });

        step("lock, unlock, change passphrase, lock");
        unmounted(0);
        let p = drive().parts[0].clone();
        if p.unlocked.is_some() {
            ctx.block_on(lock(&p.path)).unwrap();
        }
        ctx.block_on(unlock(&p.path, "correct horse 1")).unwrap();
        assert_eq!(drive().parts[0].content().fs, "btrfs");
        unmounted(0);
        ctx.block_on(lock(&p.path)).unwrap();
        ctx.block_on(change_passphrase(&p.path, "correct horse 1", "battery staple 2")).unwrap();
        assert!(ctx.block_on(unlock(&p.path, "correct horse 1")).is_err(), "the old passphrase stops working");
        ctx.block_on(unlock(&p.path, "battery staple 2")).unwrap();
        unmounted(0);
        ctx.block_on(lock(&p.path)).unwrap();

        step("startup mount entry for the exFAT partition, then remove it");
        let share = drive().parts[1].clone();
        ctx.block_on(add_fstab(&share.path, "/mnt/nixe-loop-test", "exfat", "nofail,noauto,x-gvfs-show")).unwrap();
        let fstab = std::fs::read_to_string("/etc/fstab").unwrap();
        assert!(fstab.contains("/mnt/nixe-loop-test"), "in /etc/fstab");
        assert!(drive().parts[1].fstab);
        ctx.block_on(remove_fstab(&share.path)).unwrap();
        assert!(!std::fs::read_to_string("/etc/fstab").unwrap().contains("/mnt/nixe-loop-test"), "gone from /etc/fstab");

        step("delete both partitions");
        unmounted(0);
        unmounted(1);
        for p in drive().parts {
            ctx.block_on(delete(&p.path)).unwrap();
        }
        assert!(drive().parts.is_empty());
        println!("== all done");
    }
}
