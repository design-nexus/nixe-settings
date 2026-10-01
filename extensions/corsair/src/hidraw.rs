//! Talking to one `/dev/hidraw*` node, with timeouts.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::path::Path;

pub struct Hidraw {
    file: File,
}

/// Wait for input on any of `nodes`: which one, and its report.
pub fn read_any(nodes: &mut [Hidraw], timeout_ms: i32) -> io::Result<Option<(usize, Vec<u8>)>> {
    let mut fds: Vec<libc::pollfd> =
        nodes.iter().map(|h| libc::pollfd { fd: h.file.as_raw_fd(), events: libc::POLLIN, revents: 0 }).collect();
    // SAFETY: `fds` holds valid pollfds for the duration of the call.
    let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout_ms) };
    if n < 0 {
        let e = io::Error::last_os_error();
        return if e.kind() == io::ErrorKind::Interrupted { Ok(None) } else { Err(e) };
    }
    for (i, f) in fds.iter().enumerate() {
        if f.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err(io::Error::new(io::ErrorKind::NotConnected, "device was unplugged"));
        }
        if f.revents & libc::POLLIN != 0 {
            let mut buf = [0u8; 1024];
            let len = nodes[i].file.read(&mut buf)?;
            return Ok(Some((i, buf[..len].to_vec())));
        }
    }
    Ok(None)
}

impl Hidraw {
    pub fn open(path: &Path) -> io::Result<Self> {
        Ok(Self { file: OpenOptions::new().read(true).write(true).open(path)? })
    }

    pub fn write(&mut self, report: &[u8]) -> io::Result<()> {
        self.file.write_all(report)
    }
}
