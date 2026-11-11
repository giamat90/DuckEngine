//! The Vulkan loader: opened at run time, its global functions looked up through
//! `vkGetInstanceProcAddr` (the one symbol the loader exports).
#![expect(
    unsafe_code,
    reason = "the loader is a C interface; every unsafe block has a SAFETY comment"
)]

use crate::dynlib::{Library, cast};
use crate::error::{VulkanError, check_created};
use crate::version::ApiVersion;
use crate::vk::{
    ERROR_INCOMPATIBLE_DRIVER, GetInstanceProcAddr, InstanceCreateInfo, InstanceHandle,
    LayerProperties, SUCCESS, VkResult,
};
use crate::vk_present::ExtensionProperties;
use core::ffi::{CStr, c_char, c_void};

/// The loader's file name: found in the application directory and System32 on Windows, by the
/// dynamic linker on Linux.
#[cfg(windows)]
const LOADER: &str = "vulkan-1.dll";
#[cfg(target_os = "linux")]
const LOADER: &str = "libvulkan.so.1";

/// More layers than any system has; a larger count from the loader is cut here.
const MAX_LAYERS: u32 = 1024;
/// More extensions than any loader lists; a larger count is cut here.
const MAX_EXTENSIONS: u32 = 4096;

type EnumerateInstanceVersion = unsafe extern "system" fn(*mut u32) -> VkResult;

function_table! {
    /// The functions the loader gives without an instance.
    struct GlobalFns {
        create_instance: unsafe extern "system" fn(*const InstanceCreateInfo, *const c_void, *mut InstanceHandle) -> VkResult = c"vkCreateInstance",
        enumerate_layers: unsafe extern "system" fn(*mut u32, *mut LayerProperties) -> VkResult = c"vkEnumerateInstanceLayerProperties",
        enumerate_extensions: unsafe extern "system" fn(*const c_char, *mut u32, *mut ExtensionProperties) -> VkResult = c"vkEnumerateInstanceExtensionProperties",
    }
}

/// Whether the NUL-terminated name in a driver's fixed-size field is `wanted`.
pub fn name_is(raw: &[c_char], wanted: &CStr) -> bool {
    let text = raw.iter().take_while(|byte| **byte != 0);
    text.map(|byte| byte.to_ne_bytes()[0])
        .eq(wanted.to_bytes().iter().copied())
}

/// The error for a function the loader lacks.
pub fn missing(name: &'static CStr) -> VulkanError {
    VulkanError::MissingFunction(name.to_str().unwrap_or("(non-UTF-8 name)"))
}

/// Calls the `vkDestroy...(handle, allocator)` function at `address`, if there is one.
///
/// For tearing down after a function table could not be loaded: the destroy function is looked up
/// on its own.
///
/// # Safety
/// `address` is null or the address of the destroy function for objects of `handle`'s kind, and
/// `handle` is live and not destroyed again afterwards.
pub unsafe fn destroy_found<H>(address: *mut c_void, handle: H) {
    if address.is_null() {
        return;
    }
    // SAFETY: by the contract above, the address is the destroy function for `H`.
    let destroy = unsafe { cast::<unsafe extern "system" fn(H, *const c_void)>(address) };
    // SAFETY: by the contract above, `handle` is live; the allocator is null.
    unsafe { destroy(handle, core::ptr::null()) };
}

/// A loaded Vulkan loader and its global functions.
///
/// Everything made through an `Entry` (instances, devices) holds function pointers into the
/// library, so the `Entry` must outlive them: the context owns it last.
pub struct Entry {
    get_instance_proc_addr: GetInstanceProcAddr,
    global: GlobalFns,
    enumerate_version: Option<EnumerateInstanceVersion>,
    // Last: the pointers above point into the library.
    _library: Option<Library>,
}

impl Entry {
    /// Opens the system's Vulkan loader.
    ///
    /// # Errors
    /// [`VulkanError::NoLoader`] without a loader; [`VulkanError::MissingFunction`] when it lacks
    /// `vkGetInstanceProcAddr` or a global function.
    pub fn load() -> Result<Self, VulkanError> {
        Self::load_named(LOADER)
    }

    /// Opens the shared library `name` as if it were the loader.
    fn load_named(name: &str) -> Result<Self, VulkanError> {
        let library = Library::open(name).ok_or(VulkanError::NoLoader)?;
        let address = library
            .symbol(c"vkGetInstanceProcAddr")
            .ok_or(VulkanError::MissingFunction("vkGetInstanceProcAddr"))?;
        // SAFETY: the symbol named `vkGetInstanceProcAddr` has the signature `GetInstanceProcAddr`.
        let get_instance_proc_addr = unsafe { cast::<GetInstanceProcAddr>(address.as_ptr()) };
        // The library stays loaded for the life of the process on Linux: a driver can leave
        // threads and exit handlers behind, so unloading it is a risk and keeping it costs a
        // handle. Windows closes it with the entry.
        #[cfg(target_os = "linux")]
        {
            // SAFETY: `get_instance_proc_addr` is the loader's own function, and the library is
            // never closed, so the pointers taken from it stay valid.
            let entry = unsafe { Self::from_proc_addr(get_instance_proc_addr, None) };
            core::mem::forget(library);
            entry
        }
        #[cfg(windows)]
        // SAFETY: `get_instance_proc_addr` is the loader's own function, and the entry owns the
        // library that holds it.
        unsafe {
            Self::from_proc_addr(get_instance_proc_addr, Some(library))
        }
    }

    /// An entry over the lookup function `get_instance_proc_addr`; `library` is kept loaded for
    /// as long as the entry lives.
    ///
    /// # Errors
    /// [`VulkanError::MissingFunction`] when a global function is not found.
    ///
    /// # Safety
    /// `get_instance_proc_addr` must behave as `vkGetInstanceProcAddr`: for each name it returns
    /// null or the address of a function with that name's Vulkan signature, valid for as long as
    /// the entry (and the library, when given) lives.
    pub unsafe fn from_proc_addr(
        get_instance_proc_addr: GetInstanceProcAddr,
        library: Option<Library>,
    ) -> Result<Self, VulkanError> {
        let lookup = |name: &CStr| {
            // SAFETY: a null instance is allowed for the global functions; `name` is
            // NUL-terminated.
            unsafe { get_instance_proc_addr(InstanceHandle::NULL, name.as_ptr()) }
        };
        // SAFETY: by the contract above, the addresses have the declared signatures.
        let global = unsafe { GlobalFns::load(lookup) }.map_err(missing)?;
        let version_address = lookup(c"vkEnumerateInstanceVersion");
        let enumerate_version = (!version_address.is_null()).then(|| {
            // SAFETY: by the contract above, the address is `vkEnumerateInstanceVersion`.
            unsafe { cast::<EnumerateInstanceVersion>(version_address) }
        });
        Ok(Self {
            get_instance_proc_addr,
            global,
            enumerate_version,
            _library: library,
        })
    }

    /// The highest Vulkan version the loader supports; 1.0.0 for a loader that cannot say.
    #[must_use]
    pub fn instance_version(&self) -> ApiVersion {
        let old = ApiVersion::new(1, 0, 0);
        let Some(enumerate) = self.enumerate_version else {
            return old;
        };
        let mut raw = 0_u32;
        // SAFETY: `raw` is a valid output location for the call.
        let result = unsafe { enumerate(&raw mut raw) };
        if result == SUCCESS {
            ApiVersion::from_raw(raw)
        } else {
            old
        }
    }

    /// Whether the instance layer `name` is installed.
    ///
    /// # Errors
    /// [`VulkanError::Call`] when the loader fails to list the layers.
    pub fn has_layer(&self, name: &CStr) -> Result<bool, VulkanError> {
        let call = "vkEnumerateInstanceLayerProperties";
        let mut count = 0_u32;
        // SAFETY: a null array asks for the count only.
        let result =
            unsafe { (self.global.enumerate_layers)(&raw mut count, core::ptr::null_mut()) };
        if result != SUCCESS {
            return Err(VulkanError::Call { call, result });
        }
        count = count.min(MAX_LAYERS);
        let room = usize::try_from(count).unwrap_or(0);
        let mut layers: Vec<LayerProperties> =
            (0..room).map(|_| LayerProperties::zeroed()).collect();
        // SAFETY: `layers` holds `count` records, which the loader fills (and `count` is lowered
        // to how many it wrote).
        let result = unsafe { (self.global.enumerate_layers)(&raw mut count, layers.as_mut_ptr()) };
        if result < SUCCESS {
            return Err(VulkanError::Call { call, result });
        }
        let written = usize::try_from(count).unwrap_or(0).min(room);
        Ok(layers
            .iter()
            .take(written)
            .any(|layer| name_is(&layer.layer_name, name)))
    }

    /// Whether the loader offers every instance extension in `names`.
    ///
    /// # Errors
    /// [`VulkanError::Call`] when the loader fails to list the extensions.
    pub fn has_extensions(&self, names: &[&CStr]) -> Result<bool, VulkanError> {
        let call = "vkEnumerateInstanceExtensionProperties";
        let mut count = 0_u32;
        // SAFETY: a null layer name asks for the loader's own extensions; a null array asks for
        // the count only.
        let result = unsafe {
            (self.global.enumerate_extensions)(
                core::ptr::null(),
                &raw mut count,
                core::ptr::null_mut(),
            )
        };
        if result != SUCCESS {
            return Err(VulkanError::Call { call, result });
        }
        count = count.min(MAX_EXTENSIONS);
        let room = usize::try_from(count).unwrap_or(0);
        let mut listed = vec![ExtensionProperties::zeroed(); room];
        // SAFETY: `listed` holds `count` records, which the loader fills (and lowers `count` to
        // how many it wrote).
        let result = unsafe {
            (self.global.enumerate_extensions)(
                core::ptr::null(),
                &raw mut count,
                listed.as_mut_ptr(),
            )
        };
        if result < SUCCESS {
            return Err(VulkanError::Call { call, result });
        }
        let written = usize::try_from(count).unwrap_or(0).min(room);
        let offered = &listed[..written];
        Ok(names.iter().all(|wanted| {
            offered
                .iter()
                .any(|extension| name_is(&extension.extension_name, wanted))
        }))
    }

    /// Makes an instance.
    ///
    /// # Errors
    /// [`VulkanError::NoSuitableDevice`] when the loader finds no driver (`VK_ERROR_INCOMPATIBLE_DRIVER`);
    /// [`VulkanError::Call`] with the driver's result otherwise.
    ///
    /// # Safety
    /// Every pointer in `info` (and in the structures it points to) must be valid for the call.
    pub unsafe fn create_instance(
        &self,
        info: &InstanceCreateInfo,
    ) -> Result<InstanceHandle, VulkanError> {
        let mut handle = InstanceHandle::NULL;
        // SAFETY: by the contract above `info` is valid; `handle` is a valid output location; the
        // allocator is null (the default).
        let result =
            unsafe { (self.global.create_instance)(info, core::ptr::null(), &raw mut handle) };
        if result == ERROR_INCOMPATIBLE_DRIVER {
            // The loader is there and no driver is: the same as "no suitable device" (card 6-15).
            return Err(VulkanError::NoSuitableDevice);
        }
        check_created("vkCreateInstance", result, handle.0.is_null())?;
        Ok(handle)
    }

    /// The address of the instance-level or device-level function `name` for `instance`; null
    /// when it does not exist.
    ///
    /// # Safety
    /// `instance` must be null (for global functions) or a live instance made through this entry.
    #[must_use]
    pub unsafe fn proc_addr(&self, instance: InstanceHandle, name: &CStr) -> *mut c_void {
        // SAFETY: by the contract above; `name` is NUL-terminated.
        unsafe { (self.get_instance_proc_addr)(instance, name.as_ptr()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_that_is_no_library_is_no_loader() {
        let Err(error) = Entry::load_named("duckengine-no-such-vulkan-loader") else {
            panic!("there is no such library");
        };
        assert_eq!(error, VulkanError::NoLoader);
    }

    #[cfg(windows)]
    const NOT_A_LOADER: &str = "kernel32.dll";
    #[cfg(target_os = "linux")]
    const NOT_A_LOADER: &str = "libc.so.6";

    #[test]
    fn a_library_without_vkgetinstanceprocaddr_is_a_missing_function() {
        let Err(error) = Entry::load_named(NOT_A_LOADER) else {
            panic!("a system library is no Vulkan loader");
        };
        assert_eq!(error, VulkanError::MissingFunction("vkGetInstanceProcAddr"));
    }
}
