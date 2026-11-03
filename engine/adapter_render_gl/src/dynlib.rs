//! Loading a shared library at run time: `LoadLibraryExW` on Windows, `dlopen` on Linux.
//!
//! The only code in the adapter that touches the operating system's loader. On Windows a bare
//! name is looked up in the application directory and System32 only, never the current directory
//! or `PATH`. A [`Library`] closes the library when dropped, so every function pointer taken from
//! it must not outlive it: the tables that hold such pointers own the library too.
#![expect(
    unsafe_code,
    reason = "the loader is a C interface; every unsafe block has a SAFETY comment"
)]

use core::ffi::{CStr, c_void};
use core::mem::{size_of, transmute_copy};
use core::ptr::NonNull;

/// `LOAD_LIBRARY_SEARCH_DEFAULT_DIRS`: the application directory, System32 and the directories
/// added on purpose; never the current directory or `PATH`, where a planted DLL could sit.
#[cfg(windows)]
const DEFAULT_DIRS: u32 = 0x0000_1000;

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
    /// Loads the library named `name` (a file name searched by the loader). `None` when it
    /// cannot be loaded.
    #[must_use]
    pub fn open(name: &str) -> Option<Self> {
        NonNull::new(raw_open(name)).map(Self)
    }

    /// Keeps the library loaded for the rest of the process and returns it: it is never closed.
    ///
    /// For system libraries whose state (X connections, driver threads, atexit handlers) can
    /// outlive any one context, so unloading them is a risk and keeping them costs a handle.
    /// Each call leaks one small allocation.
    #[cfg(unix)]
    #[must_use]
    pub fn leak(self) -> &'static Self {
        Box::leak(Box::new(self))
    }

    /// The address of the symbol `name`, or `None` when it is missing.
    #[must_use]
    pub fn symbol(&self, name: &CStr) -> Option<NonNull<c_void>> {
        NonNull::new(raw_symbol(self.0.as_ptr(), name))
    }
}

/// An address as a function pointer of type `F`.
///
/// The size of `F` is checked at compile time; the caller states the C signature by choosing `F`.
///
/// # Safety
/// `F` must be an `extern` function pointer type, and `address` must be the non-null address of
/// a function with exactly the signature `F`: calling the result is as unsafe as calling that
/// function.
#[must_use]
pub unsafe fn cast<F: Copy>(address: *mut c_void) -> F {
    const { assert!(size_of::<F>() == size_of::<*mut c_void>()) };
    // SAFETY: by the caller's promise `F` is a function pointer type (the same size as a data
    // pointer, checked above) for the function at `address`. Copying the bits does not read
    // through the pointer.
    unsafe { transmute_copy::<*mut c_void, F>(&address) }
}

impl Drop for Library {
    fn drop(&mut self) {
        raw_close(self.0.as_ptr());
    }
}

#[cfg(windows)]
fn raw_open(name: &str) -> *mut c_void {
    if name.contains('\0') {
        return core::ptr::null_mut();
    }
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    // SAFETY: `wide` is a NUL-terminated UTF-16 string that lives for the whole call; the file
    // handle argument must be null.
    unsafe { os::LoadLibraryExW(wide.as_ptr(), core::ptr::null_mut(), DEFAULT_DIRS) }
}

#[cfg(windows)]
fn raw_symbol(module: *mut c_void, name: &CStr) -> *mut c_void {
    // SAFETY: `module` is a live handle owned by a `Library` and `name` is NUL-terminated.
    unsafe { os::GetProcAddress(module, name.as_ptr()) }
}

#[cfg(windows)]
fn raw_close(module: *mut c_void) {
    // SAFETY: `module` came from `LoadLibraryExW` and is closed once, by `Library::drop`.
    unsafe { os::FreeLibrary(module) };
}

#[cfg(unix)]
fn raw_open(name: &str) -> *mut c_void {
    let Ok(name) = std::ffi::CString::new(name) else {
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
    #[test]
    fn a_system_library_gives_a_known_symbol_but_not_an_unknown_one() {
        let Some(lib) = Library::open("kernel32.dll") else {
            panic!("kernel32 must load");
        };
        assert!(lib.symbol(c"GetTickCount").is_some());
        assert!(lib.symbol(c"duckengine_no_such_symbol").is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_system_library_gives_a_known_symbol_but_not_an_unknown_one() {
        let Some(lib) = Library::open("libc.so.6") else {
            panic!("libc must load");
        };
        assert!(lib.symbol(c"getpid").is_some());
        assert!(lib.symbol(c"duckengine_no_such_symbol").is_none());
    }

    #[test]
    fn cast_keeps_the_address() {
        type AnyFn = unsafe extern "C" fn();
        let address = core::ptr::dangling_mut::<c_void>();
        // SAFETY: the function is never called; only its bits are compared.
        let function = unsafe { cast::<AnyFn>(address) };
        assert_eq!(function as usize, address as usize);
    }
}
