use crate::{Object, relocation::RelocatablePtr};
use paste::paste;
use std::{marker::PhantomData, ops::Deref};

/// Metaprogramming on `fn(...) -> ...` types.
///
/// This trait is not part of the stable API provided by crossmist.
#[cfg(feature = "nightly")]
pub trait FnPtr: std::ops::FnPtr {}
#[cfg(feature = "nightly")]
impl<T: std::ops::FnPtr> FnPtr for T {}

#[cfg(not(feature = "nightly"))]
mod private {
    pub trait Sealed {}
}
/// Metaprogramming on `fn(...) -> ...` types.
///
/// This trait is not part of the stable API provided by crossmist.
#[cfg(not(feature = "nightly"))]
pub trait FnPtr: Copy + private::Sealed {
    /// Convert the function pointer to a type-erased pointer.
    fn addr(self) -> usize;
}

/// An `fn(...) -> ...` implementing [`Object`].
///
/// This type enables you to pass existing `fn` and `unsafe fn` pointers between processes soundly
/// without wrapping them in [`crossmist::func`]. [`StaticFn`] dereferences into `fn(...) -> ...`,
/// so it can be called directly.
///
/// Creating [`StaticFn`] from a function pointer is `unsafe` because functions might not be
/// available in the child process if they are created in runtime by JIT compilation or by loading
/// from dynamic libraries.
///
/// All function pointers are supported on nightly. Only function pointers with up to 20 arguments
/// with no [HRTBs](https://doc.rust-lang.org/nomicon/hrtb.html) are supported without the `nightly`
/// feature flag.
///
/// # Example
///
/// ```standalone_crate
/// # use crossmist::StaticFn;
/// fn add(a: i32, b: i32) -> i32 {
///     a + b
/// }
///
/// fn main() {
///     crossmist::init();
///     entry.run(unsafe { StaticFn::new(add) }).unwrap();
/// }
///
/// #[crossmist::entrypoint]
/// fn entry(add: StaticFn<fn(i32, i32) -> i32>) {
///     assert_eq!(add(5, 7), 12);
/// }
/// ```
///
/// ```
/// # use crossmist::StaticFn;
/// let add = unsafe { StaticFn::<fn(i32, i32) -> i32>::new(|a, b| a + b) };
/// assert_eq!(add(5, 7), 12);
/// ```
///
/// ```rust
/// # use crossmist::StaticFn;
/// unsafe fn dangerous_read(p: *const i32) -> i32 {
///     p.read()
/// }
/// let dangerous_read = unsafe { StaticFn::<unsafe fn(*const i32) -> i32>::new(dangerous_read) };
/// unsafe {
///     assert_eq!(dangerous_read(&123), 123);
/// }
/// ```
///
/// The next example requires `nightly` because of references:
///
#[cfg_attr(feature = "nightly", doc = " ```")]
#[cfg_attr(not(feature = "nightly"), doc = " ```ignore")]
/// # use crossmist::StaticFn;
/// fn safe_read(p: &i32) -> i32 {
///     *p
/// }
/// let safe_read = unsafe { StaticFn::<fn(&i32) -> i32>::new(safe_read) };
/// assert_eq!(safe_read(&123), 123);
/// ```
#[derive(Clone, Copy, Debug, Object)]
#[crossmist(bound = "")]
pub struct StaticFn<F> {
    ptr: RelocatablePtr<()>,
    phantom: PhantomData<F>,
}

impl<F: FnPtr> StaticFn<F> {
    /// Create a [`StaticFn`] from a function pointer.
    ///
    /// # Safety
    ///
    /// This is safe to call if the function pointer is obtained from an `fn` item or a closure
    /// without captures.
    pub unsafe fn new(f: F) -> Self {
        Self {
            ptr: RelocatablePtr(core::ptr::with_exposed_provenance(f.addr())),
            phantom: PhantomData,
        }
    }

    /// Extract a function pointer from a [`StaticFn`].
    pub fn get_fn(self) -> F {
        unsafe { std::mem::transmute_copy::<*const (), F>(&self.ptr.0) }
    }

    const _F_IS_POINTER_SIZED: () = assert!(
        size_of::<*const ()>() == size_of::<F>(),
        "An instance of FnPtr has a size not equal to the size of *const (). This should have \
         been impossible."
    );
}

impl<F: FnPtr> Deref for StaticFn<F> {
    type Target = F;
    fn deref(&self) -> &F {
        unsafe { &*(&raw const self.ptr.0).cast::<F>() }
    }
}

macro_rules! impl_fn_pointer {
    () => {};
    ($head:tt $($tail:tt)*) => {
        paste! {
            #[cfg(not(feature = "nightly"))]
            impl<Output, $([<T $tail>]),*> private::Sealed for fn($([<T $tail>]),*) -> Output {}
            #[cfg(not(feature = "nightly"))]
            impl<Output, $([<T $tail>]),*> FnPtr for fn($([<T $tail>]),*) -> Output {
                fn addr(self) -> usize {
                    self as usize
                }
            }

            #[cfg(not(feature = "nightly"))]
            impl<Output, $([<T $tail>]),*> private::Sealed for unsafe fn($([<T $tail>]),*) -> Output {}
            #[cfg(not(feature = "nightly"))]
            impl<Output, $([<T $tail>]),*> FnPtr for unsafe fn($([<T $tail>]),*) -> Output {
                fn addr(self) -> usize {
                    self as usize
                }
            }
        }

        impl_fn_pointer!($($tail)*);
    };
}
impl_fn_pointer!(x 20 19 18 17 16 15 14 13 12 11 10 9 8 7 6 5 4 3 2 1 0);
