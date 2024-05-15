//! Serializing references to constant objects.
//!
//! Sometimes it's useful to serialize objects that *you know* are already present in each process,
//! typically because they are stored in a global static/const variable. For example, you might have
//! several unit structs implementing a trait, and you wish to serialize the `dyn Trait` without
//! wrapping it in a box. Or you might have a limited list of "actions" you wish to ask a subprocess
//! to perform, and these actions are stored in a constant array or several constant variables.
//!
//! [`StaticRef`] is a type similar to `&'static T` that implements [`Object`] and stores a plain
//! reference to the underlying value instead of serializing it as an object:
//!
//! ```rust
//! use crossmist::{StaticRef, static_ref};
//!
//! struct Configuration {
//!     meows: bool,
//!     woofs: bool,
//! }
//!
//! const CAT: Configuration = Configuration { meows: true, woofs: false };
//! const DOG: Configuration = Configuration { meows: false, woofs: true };
//!
//! #[crossmist::main]
//! fn main() {
//!     test.run(static_ref!(Configuration, CAT));
//! }
//!
//! #[crossmist::func]
//! fn test(conf: StaticRef<Configuration>) {
//!     assert_eq!(conf.meows, true);
//!     assert_eq!(conf.woofs, false);
//! }
//! ```
//!
//! Here's a more complicated example featuring `StaticRef<&'static dyn Trait>` (similar to
//! `&'static &'static dyn Trait`). The double indirection is required because `dyn Trait` is
//! unsized, and thus `&dyn Trait` is more than just a pointer, which `StaticRef` does not support
//! directly.
//!
//! ```rust
//! use crossmist::{StaticRef, static_ref};
//!
//! trait Speak {
//!     fn speak(&self) -> String;
//! }
//!
//! struct Cat;
//! impl Speak for Cat {
//!     fn speak(&self) -> String {
//!         "Meow!".to_string()
//!     }
//! }
//!
//! struct Dog;
//! impl Speak for Dog {
//!     fn speak(&self) -> String {
//!         "Woof!".to_string()
//!     }
//! }
//!
//! #[crossmist::main]
//! fn main() {
//!     test.run(static_ref!(&'static dyn Speak, &Cat));
//! }
//!
//! #[crossmist::func]
//! fn test(animal: StaticRef<&'static dyn Speak>) {
//!     assert_eq!(animal.speak(), "Meow!");
//! }
//! ```

use crate::{relocation::RelocatablePtr, Object};
use std::fmt;
use std::ops::Deref;

/// A `&'static T` implementing [`Object`].
///
/// See the documentation for [`mod@crossmist::static_ref`] for a tutorial-grade explanation.
///
/// This type can be created via one of the following two ways:
///
/// - Safely, via [`static_ref!`]
/// - Unsafely, via [`StaticRef::new_unchecked`]
///
/// # Example
///
/// ```rust
/// use crossmist::{StaticRef, static_ref};
///
/// let num = static_ref!(i32, 123);
/// assert_eq!(*num, 123);
/// ```
#[derive(Object)]
pub struct StaticRef<T: 'static> {
    ptr: RelocatablePtr<T>,
}

// Implement Clone/Copy even for T: !Clone/Copy
impl<T> Clone for StaticRef<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for StaticRef<T> {}

impl<T> StaticRef<T> {
    /// Create [`StaticRef`] from a `'static` reference.
    ///
    /// This is an unsafe function -- consider using the safe wrapper [`static_ref!`] instead.
    ///
    /// # Safety
    ///
    /// This function is safe to call if:
    /// - The referenced value must have existed since the beginning of the program execution (e.g.
    /// the return value of `Box::leak` won't work), and
    /// - The referenced value is located outside of a dynamic library.
    ///
    /// # Example
    ///
    /// ```rust
    /// use crossmist::StaticRef;
    ///
    /// static NUM: i32 = 123;
    /// let num = unsafe { StaticRef::new_unchecked(&NUM) };
    /// assert_eq!(*num, 123);
    /// ```
    pub unsafe fn new_unchecked(reference: &'static T) -> Self {
        Self {
            ptr: RelocatablePtr(reference as *const T),
        }
    }

    /// Extract the underlying reference.
    ///
    /// [`StaticRef<T>`] implements [`Deref`], so this function is only provided for completeness
    /// and should seldom be used: instead of `static_ref.get().<...>` just do `static_ref.<...>`.
    pub fn get(self) -> &'static T {
        unsafe { &*self.ptr.0 }
    }
}

impl<T: fmt::Debug> fmt::Debug for StaticRef<T> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{:?}", self.get())
    }
}

impl<T> Deref for StaticRef<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.get()
    }
}

/// Create a [`StaticRef`] safely.
///
/// This macro takes `T, value` and returns [`StaticRef<T>`] referencing the given value. The value
/// must be a compile-time constant: this is the only way to ensure soundness. If you need to
/// reference a `static`, non-compile-time constant, use [`StaticRef::new_unchecked`].
///
/// # Example
///
/// ```rust
/// use crossmist::{StaticRef, static_ref};
///
/// const NUM: i32 = 123;
/// let num = static_ref!(i32, NUM);
/// assert_eq!(*num, 123);
/// ```
#[macro_export]
macro_rules! static_ref {
    ($type:ty, $value:expr) => {{
        const VALUE: $type = $value;
        unsafe { $crate::StaticRef::new_unchecked(&VALUE) }
    }};
}
pub use static_ref;
