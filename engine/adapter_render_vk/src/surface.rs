//! The window surface: a `VkSurfaceKHR` made from the window adapter's native token, and the
//! questions a swapchain asks of it.
//!
//! The token is the `HWND` (Windows) or the X11 window id (Linux) that the window adapter hands
//! out as an opaque number. Wayland has no token and is not supported, as for the OpenGL adapter.
#![expect(
    unsafe_code,
    reason = "the surface is a C interface; every unsafe block has a SAFETY comment"
)]

use crate::context::Core;
use crate::error::{VulkanError, call_or_lost, check_created};
use crate::vk::{Bool32, InstanceHandle, PhysicalDeviceHandle, SUCCESS, VkResult};
use crate::vk_present::{EXT_SURFACE, SurfaceCapabilities, SurfaceFormat, SurfaceHandle};
use ::platform::window::NativeToken;
use core::ffi::{CStr, c_void};
use std::rc::Rc;

#[cfg(windows)]
use crate::vk_present::{
    EXT_WIN32_SURFACE, STYPE_WIN32_SURFACE_CREATE_INFO, Win32SurfaceCreateInfo,
};
#[cfg(target_os = "linux")]
use crate::vk_present::{EXT_XLIB_SURFACE, STYPE_XLIB_SURFACE_CREATE_INFO, XlibSurfaceCreateInfo};

/// The instance extensions that make surfaces on this platform.
#[cfg(windows)]
pub const SURFACE_EXTENSIONS: [&CStr; 2] = [EXT_SURFACE, EXT_WIN32_SURFACE];
/// The instance extensions that make surfaces on this platform.
#[cfg(target_os = "linux")]
pub const SURFACE_EXTENSIONS: [&CStr; 2] = [EXT_SURFACE, EXT_XLIB_SURFACE];

#[cfg(windows)]
function_table! {
    /// The surface functions (instance extensions).
    pub struct SurfaceFns {
        destroy_surface: unsafe extern "system" fn(InstanceHandle, SurfaceHandle, *const c_void) = c"vkDestroySurfaceKHR",
        get_support: unsafe extern "system" fn(PhysicalDeviceHandle, u32, SurfaceHandle, *mut Bool32) -> VkResult = c"vkGetPhysicalDeviceSurfaceSupportKHR",
        get_capabilities: unsafe extern "system" fn(PhysicalDeviceHandle, SurfaceHandle, *mut SurfaceCapabilities) -> VkResult = c"vkGetPhysicalDeviceSurfaceCapabilitiesKHR",
        get_formats: unsafe extern "system" fn(PhysicalDeviceHandle, SurfaceHandle, *mut u32, *mut SurfaceFormat) -> VkResult = c"vkGetPhysicalDeviceSurfaceFormatsKHR",
        create_platform_surface: unsafe extern "system" fn(InstanceHandle, *const Win32SurfaceCreateInfo, *const c_void, *mut SurfaceHandle) -> VkResult = c"vkCreateWin32SurfaceKHR",
    }
}

#[cfg(target_os = "linux")]
function_table! {
    /// The surface functions (instance extensions).
    pub struct SurfaceFns {
        destroy_surface: unsafe extern "system" fn(InstanceHandle, SurfaceHandle, *const c_void) = c"vkDestroySurfaceKHR",
        get_support: unsafe extern "system" fn(PhysicalDeviceHandle, u32, SurfaceHandle, *mut Bool32) -> VkResult = c"vkGetPhysicalDeviceSurfaceSupportKHR",
        get_capabilities: unsafe extern "system" fn(PhysicalDeviceHandle, SurfaceHandle, *mut SurfaceCapabilities) -> VkResult = c"vkGetPhysicalDeviceSurfaceCapabilitiesKHR",
        get_formats: unsafe extern "system" fn(PhysicalDeviceHandle, SurfaceHandle, *mut u32, *mut SurfaceFormat) -> VkResult = c"vkGetPhysicalDeviceSurfaceFormatsKHR",
        create_platform_surface: unsafe extern "system" fn(InstanceHandle, *const XlibSurfaceCreateInfo, *const c_void, *mut SurfaceHandle) -> VkResult = c"vkCreateXlibSurfaceKHR",
    }
}

/// More formats than any surface offers; a larger count is cut here.
const MAX_FORMATS: u32 = 1024;

/// A window surface. Dropping it destroys it; it holds the core, so the instance outlives it.
pub struct Surface {
    handle: SurfaceHandle,
    core: Rc<Core>,
    /// The X connection the surface was made on; it must outlive the surface.
    #[cfg(target_os = "linux")]
    _display: xlib::Display,
}

impl Surface {
    /// Makes the surface of the window named by `token`.
    ///
    /// # Errors
    /// [`VulkanError::Unsupported`] for the null token, an impossible window number, a system
    /// without the surface extensions, or (Linux) without X11; [`VulkanError::Call`] when the
    /// driver refuses.
    pub fn create(core: &Rc<Core>, token: NativeToken) -> Result<Self, VulkanError> {
        let fns = core
            .instance
            .surface_fns()
            .filter(|_| core.can_present)
            .ok_or(VulkanError::Unsupported(
                "this system or device cannot present to a window",
            ))?;
        let made = platform::make(fns, core.instance.handle(), token)?;
        Ok(Self {
            handle: made.handle,
            core: Rc::clone(core),
            #[cfg(target_os = "linux")]
            _display: made.display,
        })
    }

    /// The raw handle.
    pub fn handle(&self) -> SurfaceHandle {
        self.handle
    }

    fn fns(&self) -> Result<&SurfaceFns, VulkanError> {
        self.core
            .instance
            .surface_fns()
            .ok_or(VulkanError::MissingFunction("vkDestroySurfaceKHR"))
    }

    /// Whether `family` of the core's device can present to this surface.
    ///
    /// # Errors
    /// [`VulkanError::Call`] with the driver's result.
    pub fn supports_present(&self, family: u32) -> Result<bool, VulkanError> {
        let mut supported: Bool32 = 0;
        // SAFETY: the device was listed by the instance that made this surface; `supported` is a
        // valid output.
        let result = unsafe {
            (self.fns()?.get_support)(self.core.gpu, family, self.handle, &raw mut supported)
        };
        if result != SUCCESS {
            return Err(VulkanError::Call {
                call: "vkGetPhysicalDeviceSurfaceSupportKHR",
                result,
            });
        }
        Ok(supported != 0)
    }

    /// The surface's limits right now (they change when the window is resized).
    ///
    /// # Errors
    /// [`VulkanError::SurfaceLost`] for a lost surface; [`VulkanError::Call`] otherwise.
    pub fn capabilities(&self) -> Result<SurfaceCapabilities, VulkanError> {
        let mut caps = SurfaceCapabilities::default();
        // SAFETY: as in `supports_present`; `caps` is a valid output.
        let result =
            unsafe { (self.fns()?.get_capabilities)(self.core.gpu, self.handle, &raw mut caps) };
        if result == SUCCESS {
            Ok(caps)
        } else {
            Err(call_or_lost(
                "vkGetPhysicalDeviceSurfaceCapabilitiesKHR",
                result,
            ))
        }
    }

    /// The formats the surface offers.
    ///
    /// # Errors
    /// [`VulkanError::Call`] with the driver's result.
    pub fn formats(&self) -> Result<Vec<SurfaceFormat>, VulkanError> {
        let call = "vkGetPhysicalDeviceSurfaceFormatsKHR";
        let get = self.fns()?.get_formats;
        let mut count = 0_u32;
        // SAFETY: a null array asks for the count only.
        let result = unsafe {
            get(
                self.core.gpu,
                self.handle,
                &raw mut count,
                core::ptr::null_mut(),
            )
        };
        if result != SUCCESS {
            return Err(call_or_lost(call, result));
        }
        count = count.min(MAX_FORMATS);
        let room = usize::try_from(count).unwrap_or(0);
        let mut formats = vec![SurfaceFormat::default(); room];
        // SAFETY: `formats` holds `count` records, which the driver fills (lowering `count` to how
        // many it wrote).
        let result = unsafe {
            get(
                self.core.gpu,
                self.handle,
                &raw mut count,
                formats.as_mut_ptr(),
            )
        };
        if result < SUCCESS {
            return Err(call_or_lost(call, result));
        }
        formats.truncate(usize::try_from(count).unwrap_or(0).min(room));
        Ok(formats)
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        if let Some(fns) = self.core.instance.surface_fns() {
            // SAFETY: the surface was made on this instance, is destroyed once, here, and the
            // swapchain made from it was destroyed first (the presenter's drop order).
            unsafe {
                (fns.destroy_surface)(self.core.instance.handle(), self.handle, core::ptr::null());
            }
        }
    }
}

/// What making a platform surface returns.
struct Made {
    handle: SurfaceHandle,
    #[cfg(target_os = "linux")]
    display: xlib::Display,
}

#[cfg(windows)]
mod platform {
    use super::{
        InstanceHandle, Made, NativeToken, STYPE_WIN32_SURFACE_CREATE_INFO, SurfaceFns,
        SurfaceHandle, VulkanError, Win32SurfaceCreateInfo, c_void, check_created,
    };
    use crate::dynlib::module_instance;

    /// The window of a token, or `None` for the null token and numbers that are no address.
    fn window_of(token: NativeToken) -> Option<*const c_void> {
        let address = usize::try_from(token.0)
            .ok()
            .filter(|address| *address != 0)?;
        Some(core::ptr::without_provenance(address))
    }

    pub(super) fn make(
        fns: &SurfaceFns,
        instance: InstanceHandle,
        token: NativeToken,
    ) -> Result<Made, VulkanError> {
        let window =
            window_of(token).ok_or(VulkanError::Unsupported("no window for this token"))?;
        let info = Win32SurfaceCreateInfo {
            s_type: STYPE_WIN32_SURFACE_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            hinstance: module_instance(),
            hwnd: window,
        };
        let mut handle: SurfaceHandle = 0;
        // SAFETY: `info` is valid for the call; `handle` is a valid output. The window handle is
        // the number the window adapter gave out: Vulkan does not check it, so the caller of
        // `create_presenter` must pass a token of a window that is still alive (documented there).
        let result = unsafe {
            (fns.create_platform_surface)(
                instance,
                &raw const info,
                core::ptr::null(),
                &raw mut handle,
            )
        };
        check_created("vkCreateWin32SurfaceKHR", result, handle == 0)?;
        Ok(Made { handle })
    }
}

#[cfg(target_os = "linux")]
mod xlib {
    //! The X11 connection a Vulkan Xlib surface needs, opened from `libX11.so.6` at run time.
    use crate::dynlib::{Library, cast};
    use core::ffi::{c_char, c_int, c_void};
    use core::ptr::NonNull;

    type OpenDisplay = unsafe extern "C" fn(*const c_char) -> *mut c_void;
    type CloseDisplay = unsafe extern "C" fn(*mut c_void) -> c_int;

    /// An open X connection. Closed when dropped.
    pub struct Display {
        pub(super) pointer: NonNull<c_void>,
        close: CloseDisplay,
        // Last: `close` points into the library.
        _library: Library,
    }

    impl Display {
        /// Opens the default display, or `None` without libX11 or an X server.
        pub fn open() -> Option<Self> {
            let library = Library::open("libX11.so.6")?;
            let open = library.symbol(c"XOpenDisplay")?;
            let close = library.symbol(c"XCloseDisplay")?;
            // SAFETY: these are the documented signatures of the two Xlib functions.
            let (open, close) = unsafe {
                (
                    cast::<OpenDisplay>(open.as_ptr()),
                    cast::<CloseDisplay>(close.as_ptr()),
                )
            };
            // SAFETY: a null name opens the display named by `DISPLAY`.
            let pointer = NonNull::new(unsafe { open(core::ptr::null()) })?;
            Some(Self {
                pointer,
                close,
                _library: library,
            })
        }
    }

    impl Drop for Display {
        fn drop(&mut self) {
            // SAFETY: the display was opened by `open` and is closed once, here.
            unsafe { (self.close)(self.pointer.as_ptr()) };
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{
        InstanceHandle, Made, NativeToken, STYPE_XLIB_SURFACE_CREATE_INFO, SurfaceFns,
        SurfaceHandle, VulkanError, XlibSurfaceCreateInfo, check_created, xlib,
    };

    /// The X window id of a token: `None` for the null token and for numbers that cannot be one.
    fn window_id(token: NativeToken) -> Option<core::ffi::c_ulong> {
        core::ffi::c_ulong::try_from(token.0)
            .ok()
            .filter(|id| *id != 0)
    }

    pub(super) fn make(
        fns: &SurfaceFns,
        instance: InstanceHandle,
        token: NativeToken,
    ) -> Result<Made, VulkanError> {
        let window =
            window_id(token).ok_or(VulkanError::Unsupported("no window for this token"))?;
        let display = xlib::Display::open().ok_or(VulkanError::Unsupported("no X11 display"))?;
        let info = XlibSurfaceCreateInfo {
            s_type: STYPE_XLIB_SURFACE_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            display: display.pointer.as_ptr(),
            window,
        };
        let mut handle: SurfaceHandle = 0;
        // SAFETY: `info` is valid for the call; the display is open and kept alive by the
        // surface; the window id is the number the window adapter gave out; `handle` is a valid
        // output.
        let result = unsafe {
            (fns.create_platform_surface)(
                instance,
                &raw const info,
                core::ptr::null(),
                &raw mut handle,
            )
        };
        check_created("vkCreateXlibSurfaceKHR", result, handle == 0)?;
        Ok(Made { handle, display })
    }
}
