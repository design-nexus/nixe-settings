//! One module per group of HID++ 2.0 features; each reads and writes through a
//! device's [`Info`](super::device::Info).

pub mod battery;
pub mod buttons;
pub mod gaming;
pub mod keyboard;
pub mod pointer;
pub mod wheel;
