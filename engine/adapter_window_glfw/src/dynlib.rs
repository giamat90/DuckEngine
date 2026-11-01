//! Loading a shared library at run time: `LoadLibraryExW` on Windows, `dlopen` on Linux.
//!
//! The only code in the adapter that touches the operating system's loader. On Windows a bare
//! name is looked up in the application directory and System32 only, never the current directory
//! or `PATH`; a library given by its full path also finds its own dependencies next to it. A
//! [`Library`] closes the library when dropped, so every function pointer taken from it must not
//! outlive it.
#![expect(
    unsafe_code,
    reason = "the loader is a C interface; every unsafe block has a SAFETY comment"
)]

use core::ffi::{CStr, c_void};
use core::ptr::NonNull;

/// `LOAD_LIBRARY_SEARCH_DEFAULT_DIRS`: the application directory, System32 and the directories
/// added on purpose; never the current directory or `PATH`, where a planted DLL could sit.
#[cfg(windows)]
const DEFAULT_DIRS: u32 = 0x0000_1000;
/// `LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR`: also the folder of a library given by its full path, so
/// that its own dependencies are found next to it.
#[cfg(windows)]
const DLL_LOAD_DIR: u32 = 0x0000_0100;

#[cfg(windows)]
mod os {
    use core::ffi::{c_char, c_void};

    unsafe extern "system" {
        pub unsafe fn LoadLibraryExW(
            name: *const u16,
            file: *mut c_void,
            flags: u32,
        ) -> *mut c_void;
        pub unsafe fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
        pub unsafe fn FreeLibrary(module: *mut c_void) -> i32;
    }
}

/// `RTLD_NOW`: resolve every symbol at load, so a broken library fails here, not later.
#[cfg(unix)]
const RTLD_NOW: core::ffi::c_int = 2;

#[cfg(unix)]
mod os {
    use core::ffi::{c_char, c_int, c_void};

    unsafe extern "C" {
        pub unsafe fn dlopen(name: *const c_char, flags: c_int) -> *mut c_void;
        pub unsafe fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
        pub unsafe fn dlclose(handle: *mut c_void) -> c_int;
    }
}

/// A loaded shared library.
#[derive(Debug)]
pub struct Library(NonNull<c_void>);

impl Library {
    /// Loads the library at `path` (a file name searched by the loader, or a full path).
    /// `None` when it cannot be loaded.
    #[must_use]
    pub fn open(path: &str) -> Option<Self> {
        NonNull::new(raw_open(path)).map(Self)
    }

    /// The address of the symbol `name` (a NUL-terminated name), or `None` when it is missing.
    #[must_use]
    pub fn symbol(&self, name: &CStr) -> Option<NonNull<c_void>> {
        NonNull::new(raw_symbol(self.0.as_ptr(), name))
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        raw_close(self.0.as_ptr());
    }
}

#[cfg(windows)]
fn raw_open(path: &str) -> *mut c_void {
    if path.contains('\0') {
        return core::ptr::null_mut();
    }
    let flags = if is_full_path(path) {
        DEFAULT_DIRS | DLL_LOAD_DIR
    } else {
        DEFAULT_DIRS
    };
    let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    // SAFETY: `wide` is a NUL-terminated UTF-16 string that lives for the whole call; the file
    // handle argument must be null.
    unsafe { os::LoadLibraryExW(wide.as_ptr(), core::ptr::null_mut(), flags) }
}

/// A drive path (`C:\...`) or a UNC path; the search flags need one of these to load by path.
#[cfg(windows)]
fn is_full_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with("\\\\")
        || (bytes.len() > 2 && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/'))
}

#[cfg(windows)]
fn raw_symbol(module: *mut c_void, name: &CStr) -> *mut c_void {
    // SAFETY: `module` is a live handle owned by a `Library` and `name` is NUL-terminated.
    unsafe { os::GetProcAddress(module, name.as_ptr()) }
}

#[cfg(windows)]
fn raw_close(module: *mut c_void) {
    // SAFETY: `module` came from `LoadLibraryW` and is closed once, by `Library::drop`.
    unsafe { os::FreeLibrary(module) };
}

#[cfg(unix)]
fn raw_open(path: &str) -> *mut c_void {
    let Ok(name) = std::ffi::CString::new(path) else {
        return core::ptr::null_mut();
    };
    // SAFETY: `name` is NUL-terminated and lives for the whole call.
    unsafe { os::dlopen(name.as_ptr(), RTLD_NOW) }
}

#[cfg(unix)]
fn raw_symbol(handle: *mut c_void, name: &CStr) -> *mut c_void {
    // SAFETY: `handle` is a live handle owned by a `Library` and `name` is NUL-terminated.
    unsafe { os::dlsym(handle, name.as_ptr()) }
}

#[cfg(unix)]
fn raw_close(handle: *mut c_void) {
    // SAFETY: `handle` came from `dlopen` and is closed once, by `Library::drop`.
    unsafe { os::dlclose(handle) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_library_does_not_load() {
        assert!(Library::open("duckengine-no-such-library").is_none());
        assert!(Library::open("bad\0name").is_none());
    }

    #[cfg(windows)]
    mod win32 {
        #![expect(unsafe_code, reason = "one Win32 call on a module name, in tests")]
        use core::ffi::c_void;

        unsafe extern "system" {
            unsafe fn GetModuleHandleW(name: *const u16) -> *mut c_void;
        }

        /// The process has this module loaded.
        pub fn loaded(name: &str) -> bool {
            let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
            // SAFETY: `wide` is NUL-terminated and lives for the call.
            !unsafe { GetModuleHandleW(wide.as_ptr()) }.is_null()
        }
    }

    #[cfg(windows)]
    #[test]
    fn dropping_a_library_unloads_it() {
        let name = "msimg32.dll";
        assert!(
            !win32::loaded(name),
            "nothing else in the test process loads it"
        );
        let lib = Library::open(name);
        assert!(lib.is_some() && win32::loaded(name));
        drop(lib);
        assert!(!win32::loaded(name));
    }

    #[cfg(windows)]
    #[test]
    fn only_drive_and_unc_paths_count_as_full_paths() {
        assert!(is_full_path("C:\\dir\\glfw3.dll"));
        assert!(is_full_path("C:/dir/glfw3.dll"));
        assert!(is_full_path("\\\\server\\share\\glfw3.dll"));
        assert!(!is_full_path("glfw3.dll"));
        assert!(!is_full_path("dir\\glfw3.dll"));
        assert!(!is_full_path("C:glfw3.dll"));
        assert!(!is_full_path(""));
        assert!(!is_full_path("C:"));
    }

    #[cfg(windows)]
    #[test]
    fn a_system_library_loads_and_gives_a_known_symbol_but_not_an_unknown_one() {
        let Some(lib) = Library::open("kernel32.dll") else {
            panic!("kernel32 must load");
        };
        assert!(lib.symbol(c"GetTickCount").is_some());
        assert!(lib.symbol(c"duckengine_no_such_symbol").is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_system_library_loads_and_gives_a_known_symbol_but_not_an_unknown_one() {
        let Some(lib) = Library::open("libc.so.6") else {
            panic!("libc must load");
        };
        assert!(lib.symbol(c"getpid").is_some());
        assert!(lib.symbol(c"duckengine_no_such_symbol").is_none());
    }
}
