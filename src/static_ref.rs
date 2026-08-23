use crate::{Object, relocation::RelocatablePtr};
use std::fmt;
use std::ops::Deref;

/// A `&'static T` implementing [`Object`].
///
/// Serializing objects that are already present in each process in a `static` is wasteful. For
/// example, if you store a list of actions subprocesses may perform in a `static` array, you likely
/// want to send across references to the actions instead of serializing them and dealing with
/// temporary lifetimes. In extreme cases, the actions may not even be objects in the first place.
///
/// [`StaticRef`] is similar to `&'static T`, but implements [`Object`] by serializing a pointer. It
/// can be created from a constant value of type `T` with [`static_ref!`](crate::static_ref) and
/// sent over:
///
/// ```standalone_crate
/// use crossmist::{StaticRef, static_ref};
///
/// struct Configuration {
///     meows: bool,
///     woofs: bool,
/// }
///
/// const CAT: Configuration = Configuration { meows: true, woofs: false };
/// const DOG: Configuration = Configuration { meows: false, woofs: true };
///
/// fn main() {
///     crossmist::init();
///     test.run(static_ref!(CAT)); // sends a reference to an anonymous `static` with value `CAT`
/// }
///
/// #[crossmist::entrypoint]
/// fn test(conf: StaticRef<Configuration>) {
///     assert_eq!(conf.meows, true);
///     assert_eq!(conf.woofs, false);
/// }
/// ```
///
/// [`StaticRef`] can only point at sized data. Referencing `dyn Trait` requires double indirection
/// using `StaticRef<&'static dyn Trait>`:
///
/// ```standalone_crate
/// use crossmist::{StaticRef, static_ref};
///
/// trait Speak {
///     fn speak(&self) -> String;
/// }
///
/// struct Cat;
/// impl Speak for Cat {
///     fn speak(&self) -> String {
///         "Meow!".to_string()
///     }
/// }
///
/// struct Dog;
/// impl Speak for Dog {
///     fn speak(&self) -> String {
///         "Woof!".to_string()
///     }
/// }
///
/// fn main() {
///     crossmist::init();
///     test.run(static_ref!(&Cat as &dyn Speak));
/// }
///
/// #[crossmist::entrypoint]
/// fn test(animal: StaticRef<&'static dyn Speak>) {
///     assert_eq!(animal.speak(), "Meow!");
/// }
/// ```
///
/// [`static_ref!`](crate::static_ref) creates a new `static` and cannot reference an already
/// existing one. If that is necessary, you can use indirection:
///
/// ```rust
/// use crossmist::{StaticRef, static_ref};
///
/// static EXISTING: i32 = 123;
/// let r: StaticRef<&'static i32> = static_ref!(&EXISTING);
/// assert!(core::ptr::addr_eq(*r, &EXISTING));
/// ```
///
/// Alternatively, [`StaticRef`] can be created unsafely with [`StaticRef::new_unchecked`].
#[derive(Object)]
#[crossmist(bound = "")]
pub struct StaticRef<T> {
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
    /// This is an unsafe function -- you should almost always use
    /// [`static_ref!`](crate::static_ref) instead.
    ///
    /// # Safety
    ///
    /// The referenced value must exist since the beginning of the program execution. For example,
    /// the return value of `Box::leak` or the innards of a lazily initialized `static`s won't work.
    /// It must also be located outside of a dynamic library.
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
    pub const unsafe fn new_unchecked(reference: &'static T) -> Self {
        Self {
            ptr: RelocatablePtr(core::ptr::from_ref(reference)),
        }
    }

    /// Extract the underlying reference.
    ///
    /// [`StaticRef<T>`] implements [`Deref`], so this function should seldom be used: instead of
    /// `static_ref.get().<...>` just do `static_ref.<...>`. It is only useful to get a reference
    /// with a `'static` lifetime.
    pub const fn get<'a>(self) -> &'a T {
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
/// This macro takes a compile-time constant and returns a [`StaticRef`] to an anonymous `static`
/// initialized to that value.
///
/// # Example
///
/// ```rust
/// use crossmist::static_ref;
///
/// const NUM: i32 = 123;
/// let num = static_ref!(NUM);
/// assert_eq!(*num, 123);
/// ```
#[macro_export]
macro_rules! static_ref {
    ($value:expr) => {{
        let r = &const { $value }; // const promotion
        unsafe { $crate::StaticRef::new_unchecked(r) }
    }};
}
