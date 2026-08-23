/// Create a (possible capturing) serializable closure.
///
/// Callbacks are often used to specialize function behavior. Since Rust closures have opaque types,
/// implementing [`Object`] for built-in closures is impossible, so crossmist offers its own closure
/// constructors.
///
/// [`Object`]-compatible closures are written similarly to normal closures, but wrapped in `func!`:
///
/// ```ignore
/// crossmist::func! { |args...| body... }
/// ```
///
/// They are invoked using the [`FnOnceObject`], [`FnMutObject`], and [`FnObject`] traits, which
/// should be interpreted as [`FnOnce`] + [`Object`] and so on:
///
/// ```rust
/// use crossmist::FnOnceObject;
/// let closure = crossmist::func! { |a, b| a + b };
/// assert_eq!(closure.call_object_once((1, 2)), 3);
/// ```
///
/// On nightly Rust, if the `nightly` feature is enabled, closures can be called directly instead.
///
/// These traits are also useful for type erasure to pass callbacks over channels:
///
/// ```standalone_crate
/// use crossmist::FnOnceObject;
///
/// fn main() {
///     crossmist::init();
///     println!("{}", go.run(1, 2, crossmist::func! { |a, b| a + b }).unwrap());
/// }
///
/// #[crossmist::entrypoint]
/// fn go(x: i32, y: i32, f: Box<dyn FnOnceObject<(i32, i32), Output = i32>>) -> i32 {
///     f.call_object_once((x, y))
/// }
/// ```
///
/// # Captures
///
/// Just like normal Rust closures, crossmist closures can capture variables by value, reference, or
/// mutable reference. Unlike Rust, the captured variables need to be listed explicily, along with
/// their capture modes:
///
/// ```rust
/// let s = String::from("abc");
/// let by_ref = crossmist::func! { move(ref s) || s.len() };
///
/// let s = String::from("abc");
/// let by_ref_mut = crossmist::func! { move(ref mut s) || s.push('x') };
///
/// let s = String::from("abc");
/// let by_value = crossmist::func! { move(s) || s.into_bytes() };
/// ```
///
/// Multiple variables can be listed in the `move(...)` list separated by commas.
///
/// - A closure that captures some variables by value can only be called once.
/// - A closure that captures some variables by mutable references cannot be invoked in parallel.
/// - A closure that only captures variables by immutable reference imposes no limitations.
///
/// Reference captures work slightly differently from normal Rust. In Rust, such closures only store
/// references to captured variables, but in crossmist, the captured variables are moved into the
/// closure, so that they can then be sent over a channel together with the function itself. As
/// such, writing `func! { move(ref s) ... }` causes `s` to remain unavailable after the closure is
/// dropped.
///
/// # Example
///
/// Here is an example combining explicit capture lists with type erasure and invocation:
///
/// ```standalone_crate
/// use crossmist::func;
///
/// fn main() {
///     crossmist::init();
///     let s = String::from("abc");
///     println!("{}", go.run(crossmist::func! { move(ref s) |x| s.len() + x }).unwrap());
/// }
///
/// #[crossmist::entrypoint]
/// fn go(f: Box<dyn FnObject<(usize,), Output = usize>>) -> usize {
///     f.call_object((123,))
/// }
/// ```
///
/// Captuing more complex objects (type annotations are provided for completeness and are
/// unnecessary):
///
/// ```standalone_crate
/// # use crossmist::FnOnceObject;
/// # fn main() {
/// # crossmist::init();
/// let a = "Hello, ".to_string();
/// // a is accessible by value when the closure is executed
/// let prepend_hello: Box<dyn FnOnceObject<(&str,), Output = String>> =
///     crossmist::func! { move(a) |b| a + b };
/// assert_eq!(prepend_hello.call_object_once(("world!",)), "Hello, world!".to_string());
/// // Can only be called once. The line below fails to compile when uncommented:
/// // assert_eq!(prepend_hello.call_object_once(("world!",)), "Hello, world!".to_string());
/// # }
/// ```
///
/// ```standalone_crate
/// # use crossmist::FnMutObject;
/// # fn main() {
/// # crossmist::init();
/// let cache = vec![0, 1];
/// // cache is accessible by a mutable reference when the closure is executed
/// let mut fibonacci: Box<dyn FnMutObject<(usize,), Output = u32>> = crossmist::func! {
///     move(ref mut cache) |n| {
///         while cache.len() <= n {
///             cache.push(cache[cache.len() - 2..].iter().sum());
///         }
///         cache[n]
///     }
/// };
/// assert_eq!(fibonacci.call_object_mut((3,)), 2);
/// // Can be called multiple types, but requires unique ownership
/// assert_eq!(fibonacci.call_object_mut((6,)), 8);
/// # }
/// ```
///
/// ```standalone_crate
/// # use crossmist::FnObject;
/// # fn main() {
/// # crossmist::init();
/// let s = "Hello, world!".to_string();
/// // s is accessible by an immutable reference when the closure is executed
/// let count_occurrences: Box<dyn FnObject<(char,), Output = usize>> =
///     crossmist::func! { move(ref s) |c| s.matches(c).count() };
/// assert_eq!(count_occurrences.call_object(('o',)), 2);
/// // Can be called multiple times without a mutable reference
/// assert_eq!(count_occurrences.call_object(('e',)), 1);
/// # }
/// ```
pub use crossmist_derive::func;

use crate::Object;
use std::marker::PhantomData;

macro_rules! impl_fn {
    (
        impl[$($generic_bounds:tt)*] FnOnce<$args_ty:ty, Output = $output:ty> for $target:ty $(where[$($where:tt)*])? =
        $(#[$attr:meta])*
        |$self:tt, $args:tt| {
            $($body:tt)*
        }
    ) => {
        #[cfg(feature = "nightly")]
        impl<$($generic_bounds)*> std::ops::FnOnce<$args_ty> for $target $(where $($where)*)? {
            type Output = $output;
            $(#[$attr])*
            #[allow(unused_mut)]
            extern "rust-call" fn call_once(mut $self, $args: $args_ty) -> Self::Output {
                $($body)*
            }
        }
        #[cfg(not(feature = "nightly"))]
        impl<$($generic_bounds)*> FnOnceObject<$args_ty> for $target $(where $($where)*)? {
            type Output = $output;
            $(#[$attr])*
            #[allow(unused_mut)]
            fn call_object_once(mut $self, $args: $args_ty) -> Self::Output {
                $($body)*
            }
            fn call_object_box(self: Box<Self>, args: $args_ty) -> Self::Output {
                (*self).call_object_once(args)
            }
        }
    };

    (
        impl[$($generic_bounds:tt)*] FnMut<$args_ty:ty> for $target:ty $(where[$($where:tt)*])? =
        $(#[$attr:meta])*
        |$self:tt, $args:tt| {
            $($body:tt)*
        }
    ) => {
        #[cfg(feature = "nightly")]
        impl<$($generic_bounds)*> std::ops::FnMut<$args_ty> for $target $(where $($where)*)? {
            $(#[$attr])*
            extern "rust-call" fn call_mut(&mut $self, $args: $args_ty) -> Self::Output {
                $($body)*
            }
        }
        #[cfg(not(feature = "nightly"))]
        impl<$($generic_bounds)*> FnMutObject<$args_ty> for $target $(where $($where)*)? {
            $(#[$attr])*
            fn call_object_mut(&mut $self, $args: $args_ty) -> Self::Output {
                $($body)*
            }
        }
    };

    (
        impl[$($generic_bounds:tt)*] Fn<$args_ty:ty> for $target:ty  $(where[$($where:tt)*])?=
        $(#[$attr:meta])*
        |$self:tt, $args:tt| {
            $($body:tt)*
        }
    ) => {
        #[cfg(feature = "nightly")]
        impl<$($generic_bounds)*> std::ops::Fn<$args_ty> for $target $(where $($where)*)? {
            $(#[$attr])*
            extern "rust-call" fn call(&$self, $args: $args_ty) -> Self::Output {
                $($body)*
            }
        }
        #[cfg(not(feature = "nightly"))]
        impl<$($generic_bounds)*> FnObject<$args_ty> for $target $(where $($where)*)? {
            $(#[$attr])*
            fn call_object(&$self, $args: $args_ty) -> Self::Output {
                $($body)*
            }
        }
    };
}

/// A tuple.
///
/// Do not rely on the exact definition of this trait, as it may change depending on the enabled
/// features.
#[cfg(feature = "nightly")]
pub trait Tuple: std::marker::Tuple {}
#[cfg(feature = "nightly")]
impl<T: std::marker::Tuple> Tuple for T {}

#[cfg(not(feature = "nightly"))]
mod private {
    pub trait Sealed {}
}
/// A tuple.
///
/// Do not rely on the exact definition of this trait, as it may change depending on the enabled
/// features.
#[cfg(not(feature = "nightly"))]
pub trait Tuple: private::Sealed {}
#[cfg(not(feature = "nightly"))]
macro_rules! decl_tuple {
    () => {};
    ($head:tt $($tail:tt)*) => {
        impl<$($tail),*> private::Sealed for ($($tail,)*) {}
        impl<$($tail),*> Tuple for ($($tail,)*) {}
        decl_tuple!($($tail)*);
    };
}
#[cfg(not(feature = "nightly"))]
decl_tuple!(x T20 T19 T18 T17 T16 T15 T14 T13 T12 T11 T10 T9 T8 T7 T6 T5 T4 T3 T2 T1 T0);

/// A callable object that can be called at least once.
///
/// Do not implement this trait manually: the library gives no guarantees whether that is possible,
/// portable, or stable.
#[cfg(not(feature = "nightly"))]
pub trait FnOnceObject<Args: Tuple>: Object {
    /// Function return type.
    type Output;
    /// Invoke the function with the given argument tuple.
    ///
    /// # Example
    ///
    /// ```rust
    /// use crossmist::FnOnceObject;
    ///
    /// let s = "Hello, world!".to_string();
    /// let get_string = crossmist::func! { move(s) || s };
    ///
    /// assert_eq!(get_string.call_object_once(()), "Hello, world!");
    /// ```
    fn call_object_once(self, args: Args) -> Self::Output;
    /// Invoke a boxed function with the given argument tuple.
    ///
    /// This method is implemented as follows:
    ///
    /// ```ignore
    /// fn call_object_box(self: Box<Self>, args: Args) -> Self::Output {
    ///     (*self).call_object_once(args)
    /// }
    /// ```
    ///
    /// It enables `FnOnceObject<Args>` to be automatically implemented for
    /// `Box<dyn FnOnceObject<Args>>`.
    fn call_object_box(self: Box<Self>, args: Args) -> Self::Output;
}
/// A callable object that can be called at least once.
///
/// Do not implement this trait manually: the library gives no guarantees whether that is possible,
/// portable, or stable.
#[cfg(feature = "nightly")]
pub trait FnOnceObject<Args: Tuple>: Object + std::ops::FnOnce<Args> {
    /// Invoke the function with the given argument tuple.
    ///
    /// # Example
    ///
    /// ```rust
    /// use crossmist::FnOnceObject;
    ///
    /// let s = "Hello, world!".to_string();
    /// let get_string = crossmist::func! { move(s) || s };
    ///
    /// assert_eq!(get_string.call_object_once(()), "Hello, world!");
    /// ```
    fn call_object_once(self, args: Args) -> Self::Output;
    /// Invoke a boxed function with the given argument tuple.
    ///
    /// This method is implemented as follows:
    ///
    /// ```ignore
    /// fn call_object_box(self: Box<Self>, args: Args) -> Self::Output {
    ///     (*self).call_object_once(args)
    /// }
    /// ```
    ///
    /// It enables `FnOnceObject<Args>` to be automatically implemented for
    /// `Box<dyn FnOnceObject<Args>>`.
    fn call_object_box(self: Box<Self>, args: Args) -> Self::Output;
}
#[cfg(not(feature = "nightly"))]
impl<Args: Tuple, T: FnOnceObject<Args> + ?Sized> FnOnceObject<Args> for Box<T> {
    type Output = T::Output;
    fn call_object_once(self, args: Args) -> Self::Output {
        self.call_object_box(args)
    }
    fn call_object_box(self: Box<Self>, args: Args) -> Self::Output {
        (*self).call_object_once(args)
    }
}
#[cfg(feature = "nightly")]
impl<Args: Tuple, T: Object + std::ops::FnOnce<Args>> FnOnceObject<Args> for T {
    fn call_object_once(self, args: Args) -> Self::Output {
        self.call_once(args)
    }
    fn call_object_box(self: Box<Self>, args: Args) -> Self::Output {
        self.call_once(args)
    }
}

/// A callable object that can be called multiple times and might mutate state.
///
/// Do not implement this trait manually: the library gives no guarantees whether that is possible,
/// portable, or stable.
#[cfg(feature = "nightly")]
pub trait FnMutObject<Args: Tuple>: FnOnceObject<Args> + std::ops::FnMut<Args> {
    /// Invoke the function with the given argument tuple.
    ///
    /// # Example
    ///
    /// ```rust
    /// use crossmist::FnMutObject;
    ///
    /// let counter = 0;
    /// let mut increment = crossmist::func! {
    ///     move(ref mut counter) || { *counter += 1; *counter }
    /// };
    ///
    /// assert_eq!(increment.call_object_mut(()), 1);
    /// assert_eq!(increment.call_object_mut(()), 2);
    /// assert_eq!(increment.call_object_mut(()), 3);
    /// ```
    fn call_object_mut(&mut self, args: Args) -> Self::Output;
}
/// A callable object that can be called multiple times and might mutate state.
///
/// Do not implement this trait manually: the library gives no guarantees whether that is possible,
/// portable, or stable.
#[cfg(not(feature = "nightly"))]
pub trait FnMutObject<Args: Tuple>: FnOnceObject<Args> {
    /// Invoke the function with the given argument tuple.
    ///
    /// # Example
    ///
    /// ```rust
    /// use crossmist::FnMutObject;
    ///
    /// let counter = 0;
    /// let mut increment = crossmist::func! {
    ///     move(ref mut counter) || { *counter += 1; *counter }
    /// };
    ///
    /// assert_eq!(increment.call_object_mut(()), 1);
    /// assert_eq!(increment.call_object_mut(()), 2);
    /// assert_eq!(increment.call_object_mut(()), 3);
    /// ```
    fn call_object_mut(&mut self, args: Args) -> Self::Output;
}
#[cfg(feature = "nightly")]
impl<Args: Tuple, T: Object + std::ops::FnMut<Args>> FnMutObject<Args> for T {
    fn call_object_mut(&mut self, args: Args) -> Self::Output {
        self.call_mut(args)
    }
}

/// A callable object that can be called multiple times without mutating state.
///
/// Do not implement this trait manually: the library gives no guarantees whether that is possible,
/// portable, or stable.
#[cfg(feature = "nightly")]
pub trait FnObject<Args: Tuple>: FnMutObject<Args> + std::ops::Fn<Args> {
    /// Invoke the function with the given argument tuple.
    ///
    /// # Example
    ///
    /// ```rust
    /// use crossmist::FnObject;
    /// let add = crossmist::func! { |a, b| a + b };
    /// assert_eq!(add.call_object((5, 7)), 12);
    /// ```
    fn call_object(&self, args: Args) -> Self::Output;
}
/// A callable object that can be called multiple times without mutating state.
///
/// Do not implement this trait manually: the library gives no guarantees whether that is possible,
/// portable, or stable.
#[cfg(not(feature = "nightly"))]
pub trait FnObject<Args: Tuple>: FnMutObject<Args> {
    /// Invoke the function with the given argument tuple.
    ///
    /// # Example
    ///
    /// ```rust
    /// use crossmist::FnObject;
    /// let add = crossmist::func! { |a, b| a + b };
    /// assert_eq!(add.call_object((5, 7)), 12);
    /// ```
    fn call_object(&self, args: Args) -> Self::Output;
}
#[cfg(feature = "nightly")]
impl<Args: Tuple, T: Object + std::ops::Fn<Args>> FnObject<Args> for T {
    fn call_object(&self, args: Args) -> Self::Output {
        self.call(args)
    }
}

#[allow(missing_debug_implementations)]
#[doc(hidden)]
#[derive(Object)]
#[crossmist(bound = "ByValue: Object, ByRef: Object, ByRefMut: Object")]
pub struct Closure<Func, ByValue, ByRef, ByRefMut> {
    pub by_value: ByValue,
    pub by_ref: ByRef,
    pub by_ref_mut: ByRefMut,
    pub _phantom: PhantomData<Func>,
}

impl<Func, ByValue, ByRef, ByRefMut> Closure<Func, ByValue, ByRef, ByRefMut> {
    // Has to be safe for macros to not wrap user code in `unsafe`
    pub fn unsafe_new<Args, Output>(
        _func: Func,
        by_value: ByValue,
        by_ref: ByRef,
        by_ref_mut: ByRefMut,
    ) -> Self
    where
        // necessary so that the borrowed types get inferred
        Func: for<'a> Fn(ByValue, &'a ByRef, &'a mut ByRefMut, Args) -> Output,
    {
        Self {
            by_value,
            by_ref,
            by_ref_mut,
            _phantom: PhantomData,
        }
    }

    pub fn conjure(&self) -> Func {
        unsafe { core::ptr::dangling::<Func>().read() }
    }
}

impl_fn! {
    impl[ByValue: Object, ByRef: Object, ByRefMut: Object, Args: Tuple, Output, Func: for<'a> Fn(ByValue, &'a ByRef, &'a mut ByRefMut, Args) -> Output] FnOnce<Args, Output = Output> for Closure<Func, ByValue, ByRef, ByRefMut> =
    |self, args| {
        (self.conjure())(self.by_value, &self.by_ref, &mut self.by_ref_mut, args)
    }
}

impl_fn! {
    impl[ByRef: Object, ByRefMut: Object, Args: Tuple, Output, Func: for<'a> Fn((), &'a ByRef, &'a mut ByRefMut, Args) -> Output] FnMut<Args> for Closure<Func, (), ByRef, ByRefMut> =
    |self, args| {
        (self.conjure())((), &self.by_ref, &mut self.by_ref_mut, args)
    }
}

impl_fn! {
    impl[ByRef: Object, Args: Tuple, Output, Func: for<'a> Fn((), &'a ByRef, &'a mut (), Args) -> Output] Fn<Args> for Closure<Func, (), ByRef, ()> =
    |self, args| {
        (self.conjure())((), &self.by_ref, &mut (), args)
    }
}
