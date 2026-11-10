//! Function tables: C functions looked up by name, checked as a whole.
//!
//! A table is a struct of function pointers, each with its C signature as its type. `load` asks a
//! lookup closure for every name, in declaration order, and refuses the whole table with the name
//! of the first function that is missing, so a driver that lacks one function is found when the
//! device is made, not in the middle of a frame. (The same pattern as the OpenGL adapter's; a
//! shared crate would need an architecture rule, so the few lines are repeated.)

#![expect(
    unsafe_code,
    reason = "a function table casts addresses to function pointers; every unsafe block has a SAFETY comment"
)]

use crate::dynlib::cast;
use core::ffi::{CStr, c_void};

/// Declares a struct of C function pointers and its `load`.
///
/// ```text
/// function_table! {
///     /// Docs.
///     struct Name {
///         field: unsafe extern "system" fn(i32) = c"cSymbolName",
///     }
/// }
/// ```
macro_rules! function_table {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $($field:ident : $ty:ty = $symbol:literal,)+
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy)]
        $vis struct $name {
            $($field: $ty,)+
        }

        impl $name {
            /// Looks every function up; `Err` carries the name of the first one that is missing.
            ///
            /// # Safety
            /// Every address `lookup` returns must be the address of a function with the C
            /// signature declared for that name in the table.
            $vis unsafe fn load(
                mut lookup: impl FnMut(&::core::ffi::CStr) -> *mut ::core::ffi::c_void,
            ) -> Result<Self, &'static ::core::ffi::CStr> {
                Ok(Self {
                    // SAFETY: the caller of `load` promises the signatures.
                    $($field: unsafe { $crate::lookup::resolve::<$ty>(&mut lookup, $symbol) }?,)+
                })
            }
        }
    };
}

/// The address `lookup` gives for `symbol` as a function of type `F`, or the name when the lookup
/// finds nothing.
///
/// # Safety
/// `F` must be an `extern` function pointer type, and when `lookup` returns an address for
/// `symbol` it must be the address of a function with exactly the signature `F`.
pub unsafe fn resolve<F: Copy>(
    lookup: &mut impl FnMut(&CStr) -> *mut c_void,
    symbol: &'static CStr,
) -> Result<F, &'static CStr> {
    let address = lookup(symbol);
    if address.is_null() {
        Err(symbol)
    } else {
        // SAFETY: the caller promises `F` is the signature of the function at `address`.
        Ok(unsafe { cast::<F>(address) })
    }
}

#[cfg(test)]
mod tests {
    use core::ffi::{CStr, c_void};
    use core::ptr::{dangling_mut, null_mut};

    function_table! {
        /// Two functions, for the tests.
        struct Pair {
            first: unsafe extern "C" fn() = c"first",
            second: unsafe extern "C" fn(i32) -> i32 = c"second",
        }
    }

    fn found(_: &CStr) -> *mut c_void {
        dangling_mut::<c_void>()
    }

    fn without_second(name: &CStr) -> *mut c_void {
        if name == c"second" {
            null_mut()
        } else {
            dangling_mut::<c_void>()
        }
    }

    #[test]
    fn a_table_loads_when_every_name_is_found() {
        // SAFETY: the addresses are never called; only their bits are checked.
        let pair = unsafe { Pair::load(found) };
        assert!(pair.is_ok());
    }

    #[test]
    fn a_table_names_the_first_function_that_is_missing() {
        // SAFETY: the addresses are never called.
        let Err(missing) = (unsafe { Pair::load(without_second) }) else {
            panic!("second is missing, the table must not load");
        };
        assert_eq!(missing, c"second");
    }

    #[test]
    fn a_table_asks_for_each_name_once_in_declaration_order() {
        let mut asked = Vec::new();
        // SAFETY: the addresses are never called.
        let _ = unsafe {
            Pair::load(|name| {
                asked.push(name.to_owned());
                dangling_mut::<c_void>()
            })
        };
        assert_eq!(asked, [c"first".to_owned(), c"second".to_owned()]);
    }

    #[test]
    fn a_table_keeps_the_address_it_was_given() {
        let address = dangling_mut::<c_void>();
        // SAFETY: the addresses are never called.
        let Ok(pair) = (unsafe { Pair::load(|_| address) }) else {
            panic!("all found");
        };
        let want = address.addr();
        assert_eq!((pair.first as usize, pair.second as usize), (want, want));
    }
}
