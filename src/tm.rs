//! Thin layer over macOS Time Machine exclusions.
//!
//! The scan reads and writes the sticky exclusion mark directly: reading is an extended
//! attribute lookup, writing is the CoreServices call that `tmutil addexclusion` and
//! cargo use underneath. Both take well under a millisecond, while spawning `tmutil`
//! takes ~60 ms to check and several seconds to write. The mark is byte-for-byte the
//! one `tmutil` writes, so Time Machine and Spotlight see no difference.

use std::path::Path;
use std::process::Command;

/// The sticky exclusion mark (an extended attribute holding a small binary plist).
pub const EXCLUSION_XATTR: &str = "com.apple.metadata:com_apple_backup_excludeItem";

/// True if `path` itself carries the sticky exclusion mark.
///
/// Unlike `tmutil isexcluded`, this ignores system-wide exclusions (such as `/tmp`) and
/// exclusions inherited from a parent folder.
pub fn has_exclusion_mark(path: &Path) -> bool {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let (Ok(path), Ok(name)) = (
        CString::new(path.as_os_str().as_bytes()),
        CString::new(EXCLUSION_XATTR),
    ) else {
        return false;
    };
    // SAFETY: both strings are NUL-terminated; a null buffer only asks for the size.
    unsafe { ffi::getxattr(path.as_ptr(), name.as_ptr(), std::ptr::null_mut(), 0, 0, 0) >= 0 }
}

/// Sets (`exclude = true`) or clears the sticky exclusion mark.
/// Returns true if the mark is in the requested state afterwards.
pub fn set_exclusion_mark(path: &Path, exclude: bool) -> bool {
    use std::os::unix::ffi::OsStrExt;

    let Ok(path) = std::path::absolute(path) else {
        return false;
    };
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: `bytes` outlives the call, the URL is checked for null and released once.
    let status = unsafe {
        let url = ffi::CFURLCreateFromFileSystemRepresentation(
            std::ptr::null(),
            bytes.as_ptr(),
            bytes.len() as isize,
            path.is_dir() as u8,
        );
        if url.is_null() {
            return false;
        }
        let status = ffi::CSBackupSetItemExcluded(url, exclude as u8, 0);
        ffi::CFRelease(url);
        status
    };
    status == 0 && has_exclusion_mark(&path) == exclude
}

/// Checks if a path is excluded from Time Machine backups, as `tmutil isexcluded` sees it
/// (including system-wide and inherited exclusions).
pub fn is_excluded(path: &Path) -> bool {
    match Command::new("tmutil").arg("isexcluded").arg(path).output() {
        Ok(output) => String::from_utf8_lossy(&output.stdout).contains("[Excluded]"),
        Err(_) => false, // Failed to run tmutil
    }
}

mod ffi {
    use std::ffi::{c_char, c_int, c_void};

    pub type CFURLRef = *const c_void;

    extern "C" {
        /// macOS getxattr(2); options 0 follows symlinks, like `tmutil`.
        pub fn getxattr(
            path: *const c_char,
            name: *const c_char,
            value: *mut c_void,
            size: usize,
            position: u32,
            options: c_int,
        ) -> isize;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        pub fn CFURLCreateFromFileSystemRepresentation(
            allocator: *const c_void,
            buffer: *const u8,
            buf_len: isize,
            is_directory: u8,
        ) -> CFURLRef;
        pub fn CFRelease(cf: *const c_void);
    }

    #[link(name = "CoreServices", kind = "framework")]
    extern "C" {
        pub fn CSBackupSetItemExcluded(item: CFURLRef, exclude: u8, exclude_by_path: u8) -> i32;
    }
}
