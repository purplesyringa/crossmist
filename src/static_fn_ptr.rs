use crate::{Object, relocation::RelocatablePtr};
use std::{marker::PhantomData, ops::Deref};

#[cfg(feature = "nightly")]
use core::ops::FnPtr;
#[cfg(not(feature = "nightly"))]
use polyfill::FnPtr;

/// An `fn(...) -> ...` implementing [`Object`].
///
/// This type enables you to pass existing `fn` and `unsafe fn` pointers between processes soundly
/// without wrapping them in [`crossmist::func`]. [`StaticFnPtr`] dereferences to `fn(...) -> ...`,
/// so it can be called directly.
///
/// Creating [`StaticFnPtr`] from a function pointer is `unsafe` because functions might not be
/// available in the child process if they are created in runtime by JIT compilation or by loading
/// from dynamic libraries.
///
/// # Supported types
///
/// All function pointers are supported on nightly via the unstable [`FnPtr`](core::ops::FnPtr)
/// trait. Only function pointers with up to 20 arguments with no
/// [HRTBs](https://doc.rust-lang.org/nomicon/hrtb.html) are supported without the `nightly` feature
/// flag.
///
/// # Example
///
/// ```standalone_crate
/// # use crossmist::StaticFnPtr;
/// fn add(a: i32, b: i32) -> i32 {
///     a + b
/// }
///
/// fn main() {
///     crossmist::init();
///     entry.run(unsafe { StaticFnPtr::new_unchecked(add) }).unwrap();
/// }
///
/// #[crossmist::entrypoint]
/// fn entry(add: StaticFnPtr<fn(i32, i32) -> i32>) {
///     assert_eq!(add(5, 7), 12);
/// }
/// ```
///
/// ```
/// # use crossmist::StaticFnPtr;
/// let add = unsafe { StaticFnPtr::<fn(i32, i32) -> i32>::new_unchecked(|a, b| a + b) };
/// assert_eq!(add(5, 7), 12);
/// ```
///
/// ```rust
/// # use crossmist::StaticFnPtr;
/// unsafe fn dangerous_read(p: *const i32) -> i32 {
///     p.read()
/// }
/// let dangerous_read = unsafe {
///     StaticFnPtr::<unsafe fn(*const i32) -> i32>::new_unchecked(dangerous_read)
/// };
/// unsafe {
///     assert_eq!(dangerous_read(&123), 123);
/// }
/// ```
///
/// The next example requires `nightly` because of references:
///
#[cfg_attr(feature = "nightly", doc = " ```")]
#[cfg_attr(not(feature = "nightly"), doc = " ```ignore")]
/// # use crossmist::StaticFnPtr;
/// fn safe_read(p: &i32) -> i32 {
///     *p
/// }
/// let safe_read = unsafe { StaticFnPtr::<fn(&i32) -> i32>::new_unchecked(safe_read) };
/// assert_eq!(safe_read(&123), 123);
/// ```
#[derive(Clone, Copy, Debug, Object)]
#[crossmist(bound = "")]
pub struct StaticFnPtr<F> {
    ptr: RelocatablePtr<()>,
    phantom: PhantomData<F>,
}

impl<F: FnPtr> StaticFnPtr<F> {
    /// Create a [`StaticFnPtr`] from a function pointer.
    ///
    /// # Safety
    ///
    /// This is safe to call if the function pointer is obtained from an `fn` item or a closure
    /// without captures.
    pub unsafe fn new_unchecked(f: F) -> Self {
        Self {
            ptr: RelocatablePtr(core::ptr::with_exposed_provenance(f.addr())),
            phantom: PhantomData,
        }
    }

    /// Extract a function pointer from a [`StaticFnPtr`].
    ///
    /// This method usually shouldn't be used, since the [`StaticFnPtr`] can be called directly.
    pub fn get(self) -> F {
        unsafe { std::mem::transmute_copy::<*const (), F>(&self.ptr.0) }
    }

    const _F_IS_POINTER_SIZED: () = assert!(
        size_of::<*const ()>() == size_of::<F>(),
        "An instance of FnPtr has a size not equal to the size of *const (). This should have \
         been impossible."
    );
}

impl<F: FnPtr> Deref for StaticFnPtr<F> {
    type Target = F;
    fn deref(&self) -> &F {
        unsafe { &*(&raw const self.ptr.0).cast::<F>() }
    }
}

#[cfg(not(feature = "nightly"))]
mod polyfill {
    use paste::paste;

    #[cfg(not(feature = "nightly"))]
    pub trait FnPtr: Copy {
        fn addr(self) -> usize;
    }

    #[cfg(not(feature = "nightly"))]
    macro_rules! impl_fn_pointer {
        () => {};
        ($head:tt $($tail:tt)*) => {
            paste! {
                impl<Output, $([<T $tail>]),*> FnPtr for fn($([<T $tail>]),*) -> Output {
                    fn addr(self) -> usize {
                        self as usize
                    }
                }

                impl<Output, $([<T $tail>]),*> FnPtr for unsafe fn($([<T $tail>]),*) -> Output {
                    fn addr(self) -> usize {
                        self as usize
                    }
                }
            }

            impl_fn_pointer!($($tail)*);
        };
    }
    #[cfg(not(feature = "nightly"))]
    impl_fn_pointer!(x 20 19 18 17 16 15 14 13 12 11 10 9 8 7 6 5 4 3 2 1 0);
}
