//! Filesystem totals for the header, like `df`.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub struct FsUsage {
    pub total: u64,
    /// Free blocks, including those reserved for root.
    pub free: u64,
    /// Free space available to normal users.
    pub available: u64,
}

impl FsUsage {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.free)
    }
}

pub fn usage(path: &Path) -> Option<FsUsage> {
    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: statvfs only writes into the struct we hand it.
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c_path.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let block = s.f_frsize as u64;
    Some(FsUsage {
        total: s.f_blocks as u64 * block,
        free: s.f_bfree as u64 * block,
        available: s.f_bavail as u64 * block,
    })
}
