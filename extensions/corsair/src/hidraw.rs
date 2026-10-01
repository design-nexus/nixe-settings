//! Talking to one `/dev/hidraw*` node, with timeouts.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::path::Path;

pub struct Hidraw {
    file: File,
}

impl Hidraw {
    pub fn open(path: &Path) -> io::Result<Self> {
        Ok(Self { file: OpenOptions::new().read(true).write(true).open(path)? })
    }

    pub fn write(&mut self, report: &[u8]) -> io::Result<()> {
        self.file.write_all(report)
    }

    /// One input report, or `None` if nothing arrives in time.
    pub fn read(&mut self, timeout_ms: i32) -> io::Result<Option<Vec<u8>>> {
        let mut pfd = libc::pollfd { fd: self.file.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        // SAFETY: one valid pollfd for the duration of the call.
        let n = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
        if n < 0 {
            let e = io::Error::last_os_error();
            return if e.kind() == io::ErrorKind::Interrupted { Ok(None) } else { Err(e) };
        }
        if n == 0 {
            return Ok(None);
        }
        if pfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err(io::Error::new(io::ErrorKind::NotConnected, "device was unplugged"));
        }
        let mut buf = [0u8; 1024];
        let len = self.file.read(&mut buf)?;
        Ok(Some(buf[..len].to_vec()))
    }
}
