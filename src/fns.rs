//! Utilities for passing function callbacks between processes.
//!
//! It is common to use callbacks to specialize function behavior. Capturing lambdas play an
//! especially big role in this. They are, however, of complex opaque types that cannot be
//! inspected. Therefore, passing lambdas is not just complicated because they would have to be of
//! type `dyn Object + Fn() -> ()`, which Rust does not support at the moment, but downright
//! impossible in case of captures.
//!
//! To fix the following code:
//!
//! ```compile_fail
//! use crossmist::Object;
//!
//! fn main() {
//!     crossmist::init();
//!     let x = 7;
//!     println!("{}", go.run(5, Box::new(|y| x + y)).unwrap());
//! }
//!
//! #[crossmist::func]
//! fn go(x: i32, f: Box<dyn Object + Fn(i32) -> i32>) -> i32 {
//!     f(x)
//! }
//! ```
//!
//! ...we have to use a macro, and also a different invocation syntax:
//!
//! ```standalone_crate
//! use crossmist::{FnObject, lambda};
//!
//! fn main() {
//!     crossmist::init();
//!     let x = 7;
//!     println!("{}", go.run(5, lambda! { move(x: i32) |y: i32| -> i32 { x + y } }).unwrap());
//! }
//!
//! #[crossmist::func]
//! fn go(x: i32, f: Box<dyn FnObject<(i32,), Output = i32>>) -> i32 {
//!     f.call_object((x,))
//! }
//! ```
//!
//! The macro syntax is somewhat similar to that of capturing lambdas. `call_object` is similar to
//! [`std::ops::Fn::call`]. If you're using nightly Rust, you can directly do `f(x)` if you opt in
//! by enabling the `nightly` feature.
//!
//! Another complication is when the callback should capture a non-copyable value (e.g. [`Box`]) and
//! then be called multiple times. This cannot be detected automatically, so slightly different
//! syntax is used:
//!
//! ```standalone_crate
//! use crossmist::{FnObject, lambda};
//!
//! fn main() {
//!     crossmist::init();
//!     let x = Box::new(7);
//!     println!("{}", go.run(5, lambda! { move(ref x: Box<i32>) |y: i32| -> i32 { **x + y } }).unwrap());
//! }
//!
//! #[crossmist::func]
//! fn go(x: i32, f: Box<dyn FnObject<(i32,), Output = i32>>) -> i32 {
//!     f.call_object((x,))
//! }
//! ```
//!
//! Similarly, `ref mut x` can be used if the object is to be modified. Note that this still moves
//! `x` into the lambda.
//!
//! Under the hood, the macro uses currying, replacing `|y| x + y` with `|x, y| x + y` with a
//! pre-determined `x` variable, and makes `|x, y| x + y` a callable [`Object`] by using `#[func]`:
//!
//! ```standalone_crate
//! use crossmist::{BindValue, FnObject};
//!
//! fn main() {
//!     crossmist::init();
//!
//!     #[crossmist::func]
//!     fn add(x: i32, y: i32) -> i32 {
//!         x + y
//!     }
//!
//!     let x = 7;
//!     println!("{}", go.run(5, Box::new(add.bind_value(x))).unwrap());
//! }
//!
//! #[crossmist::func]
//! fn go(x: i32, f: Box<dyn FnObject<(i32,), Output = i32>>) -> i32 {
//!     f.call_object((x,))
//! }
//! ```

use crate::{Object, relocation::RelocatablePtr};
use paste::paste;
use std::marker::PhantomData;
use std::ops::Deref;

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

#[allow(missing_debug_implementations)]
#[doc(hidden)]
#[derive(Object)]
pub struct CallWrapper<T: Object>(pub T);

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

impl<T: Object> Deref for CallWrapper<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

#[doc(hidden)]
pub trait InternalFnOnce<Args>: Object {
    type Output;
    fn call_object_once(self, args: Args) -> Self::Output;
}
impl_fn! {
    impl[Args: Tuple, T: InternalFnOnce<Args>] FnOnce<Args, Output = T::Output> for CallWrapper<T> =
    |self, args| {
        self.0.call_object_once(args)
    }
}

#[doc(hidden)]
pub trait InternalFnMut<Args>: InternalFnOnce<Args> {
    fn call_object_mut(&mut self, args: Args) -> Self::Output;
}
impl_fn! {
    impl[Args: Tuple, T: InternalFnMut<Args>] FnMut<Args> for CallWrapper<T> = |self, args| {
        self.0.call_object_mut(args)
    }
}

#[doc(hidden)]
pub trait InternalFn<Args>: InternalFnMut<Args> {
    fn call_object(&self, args: Args) -> Self::Output;
}
impl_fn! {
    impl[Args: Tuple, T: InternalFn<Args>] Fn<Args> for CallWrapper<T> = |self, args| {
        self.0.call_object(args)
    }
}

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
    /// use crossmist::{FnOnceObject, lambda};
    ///
    /// let s = "Hello, world!".to_string();
    /// let mut increment = lambda! { move(s: String) || -> String { s } };
    ///
    /// assert_eq!(increment.call_object_once(()), "Hello, world!");
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
    /// use crossmist::{FnOnceObject, lambda};
    ///
    /// let s = "Hello, world!".to_string();
    /// let mut increment = lambda! { move(s: String) || -> String { s } };
    ///
    /// assert_eq!(increment.call_object_once(()), "Hello, world!");
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
impl<Args: Tuple, T: FnOnceObject<Args> + ?Sized> FnOnceObject<Args> for Box<T>
where
    Box<T>: Object,
{
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
    /// use crossmist::{FnMutObject, lambda};
    ///
    /// let counter = 0;
    /// let mut increment = lambda! {
    ///     move(ref mut counter: i32) || -> i32 { *counter += 1; *counter }
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
    /// use crossmist::{FnMutObject, lambda};
    ///
    /// let counter = 0;
    /// let mut increment = lambda! {
    ///     move(ref mut counter: i32) || -> i32 { *counter += 1; *counter }
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
    ///
    /// #[crossmist::func]
    /// fn add(a: i32, b: i32) -> i32 {
    ///     a + b
    /// }
    ///
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
    ///
    /// #[crossmist::func]
    /// fn add(a: i32, b: i32) -> i32 {
    ///     a + b
    /// }
    ///
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

#[doc(hidden)]
pub trait BindValue<Head: Object, Tail>: Object + Sized {
    fn bind_value(self, head: Head) -> BoundValue<Self, Head>;
}
#[doc(hidden)]
pub trait BindMut<Head: Object, Tail>: Object + Sized {
    fn bind_mut(self, head: Head) -> BoundMut<Self, Head>;
}
#[doc(hidden)]
pub trait BindRef<Head: Object, Tail>: Object + Sized {
    fn bind_ref(self, head: Head) -> BoundRef<Self, Head>;
}

#[allow(missing_debug_implementations)]
#[doc(hidden)]
#[derive(Object)]
pub struct BoundValue<Func: Object, Head: Object> {
    pub func: Func,
    pub head: Head,
}
#[allow(missing_debug_implementations)]
#[doc(hidden)]
#[derive(Object)]
pub struct BoundMut<Func: Object, Head: Object> {
    pub func: Func,
    pub head: Head,
}
#[allow(missing_debug_implementations)]
#[doc(hidden)]
#[derive(Object)]
pub struct BoundRef<Func: Object, Head: Object> {
    pub func: Func,
    pub head: Head,
}

macro_rules! reverse {
    ([$($acc:tt)*]) => { ($($acc)*) };
    ([$($acc:tt)*] $single:tt) => { reverse!([$single, $($acc)*]) };
    ([$($acc:tt)*] $head:tt, $($tail:tt),*) => { reverse!([$head, $($acc)*] $($tail),*) };
}

macro_rules! decl_fn {
    () => {};

    ($head:tt $($tail:tt)*) => {
        decl_fn!($($tail)*);

        paste! {
            impl<[<T $head>]: Object $(, [<T $tail>])*, Func: FnOnceObject<([<T $head>], $([<T $tail>]),*)>> BindValue<[<T $head>], ($([<T $tail>],)*)> for Func {
                fn bind_value(self, head: [<T $head>]) -> BoundValue<Self, [<T $head>]> {
                    BoundValue {
                        func: self,
                        head,
                    }
                }
            }
            impl<'a, [<T $head>]: 'a + Object $(, [<T $tail>])*, Func: FnOnceObject<(&'a mut [<T $head>], $([<T $tail>]),*)>> BindMut<[<T $head>], ($([<T $tail>],)*)> for Func {
                fn bind_mut(self, head: [<T $head>]) -> BoundMut<Self, [<T $head>]> {
                    BoundMut {
                        func: self,
                        head,
                    }
                }
            }
            impl<'a, [<T $head>]: 'a + Object $(, [<T $tail>])*, Func: FnOnceObject<(&'a [<T $head>], $([<T $tail>]),*)>> BindRef<[<T $head>], ($([<T $tail>],)*)> for Func {
                fn bind_ref(self, head: [<T $head>]) -> BoundRef<Self, [<T $head>]> {
                    BoundRef {
                        func: self,
                        head,
                    }
                }
            }

            impl_fn! {
                impl[[<T $head>]: Object $(, [<T $tail>])*, Func: FnOnceObject<([<T $head>], $([<T $tail>]),*)>] FnOnce<($([<T $tail>],)*), Output = Func::Output> for BoundValue<Func, [<T $head>]> =
                #[allow(unused_variables)]
                |self, args| {
                    self.func.call_object_once(reverse!([] $((args.$tail),)* (self.head)))
                }
            }
            impl_fn! {
                impl[[<T $head>]: Copy + Object $(, [<T $tail>])*, Func: FnMutObject<([<T $head>], $([<T $tail>]),*)>] FnMut<($([<T $tail>],)*)> for BoundValue<Func, [<T $head>]> =
                #[allow(unused_variables)]
                |self, args| {
                    self.func.call_object_mut(reverse!([] $((args.$tail),)* (self.head)))
                }
            }
            impl_fn! {
                impl[[<T $head>]: Copy + Object $(, [<T $tail>])*, Func: FnObject<([<T $head>], $([<T $tail>]),*)>] Fn<($([<T $tail>],)*)> for BoundValue<Func, [<T $head>]> =
                #[allow(unused_variables)]
                |self, args| {
                    self.func.call_object(reverse!([] $((args.$tail),)* (self.head)))
                }
            }

            impl_fn! {
                impl[[<T $head>]: Object $(, [<T $tail>])*, Output, Func: for<'a> FnOnceObject<(&'a mut [<T $head>], $([<T $tail>]),*), Output = Output>] FnOnce<($([<T $tail>],)*), Output = Output> for BoundMut<Func, [<T $head>]> =
                #[allow(unused_variables)]
                |self, args| {
                    self.func.call_object_once(reverse!([] $((args.$tail),)* (&mut self.head)))
                }
            }
            impl_fn! {
                impl[[<T $head>]: Object $(, [<T $tail>])*, Output, Func: for<'a> FnMutObject<(&'a mut [<T $head>], $([<T $tail>]),*), Output = Output>] FnMut<($([<T $tail>],)*)> for BoundMut<Func, [<T $head>]> =
                #[allow(unused_variables)]
                |self, args| {
                    self.func.call_object_mut(reverse!([] $((args.$tail),)* (&mut self.head)))
                }
            }

            impl_fn! {
                impl[[<T $head>]: Object $(, [<T $tail>])*, Output, Func: for<'a> FnOnceObject<(&'a [<T $head>], $([<T $tail>]),*), Output = Output>] FnOnce<($([<T $tail>],)*), Output = Output> for BoundRef<Func, [<T $head>]> =
                #[allow(unused_variables)]
                |self, args| {
                    self.func.call_object_once(reverse!([] $((args.$tail),)* (&self.head)))
                }
            }
            impl_fn! {
                impl[[<T $head>]: Object $(, [<T $tail>])*, Output, Func: for<'a> FnMutObject<(&'a [<T $head>], $([<T $tail>]),*), Output = Output>] FnMut<($([<T $tail>],)*)> for BoundRef<Func, [<T $head>]> =
                #[allow(unused_variables)]
                |self, args| {
                    self.func.call_object_mut(reverse!([] $((args.$tail),)* (&self.head)))
                }
            }
            impl_fn! {
                impl[[<T $head>]: Object $(, [<T $tail>])*, Output, Func: for<'a> FnObject<(&'a [<T $head>], $([<T $tail>]),*), Output = Output>] Fn<($([<T $tail>],)*)> for BoundRef<Func, [<T $head>]> =
                #[allow(unused_variables)]
                |self, args| {
                    self.func.call_object(reverse!([] $((args.$tail),)* (&self.head)))
                }
            }
        }
    }
}

decl_fn!(x 19 18 17 16 15 14 13 12 11 10 9 8 7 6 5 4 3 2 1 0);

/// Metaprogramming on `fn(...) -> ...` types.
///
/// This trait is not part of the stable API provided by crossmist.
#[cfg(feature = "nightly")]
pub trait FnPtr: std::ops::FnPtr {}
#[cfg(feature = "nightly")]
impl<T: std::ops::FnPtr> FnPtr for T {}

#[cfg(not(feature = "nightly"))]
mod fn_ptr_private {
    pub trait Sealed {}
}
/// Metaprogramming on `fn(...) -> ...` types.
///
/// This trait is not part of the stable API provided by crossmist.
#[cfg(not(feature = "nightly"))]
pub trait FnPtr: Copy + fn_ptr_private::Sealed {
    /// Convert the function pointer to a type-erased pointer.
    fn addr(self) -> usize;
}

/// A wrapper for `fn(...) -> ...` implementing `Object`.
///
/// This type enables you to pass `fn` and `unsafe fn` pointers between processes soundly without
/// requiring [`lambda`] or [`crossmist::func`].
///
/// Creating the wrapper from a function pointer is `unsafe` because functions might not be
/// available in the child process if they were created in runtime by JIT compilation or alike.
///
/// All function pointers are supported on nightly. Only function pointers with up to 20 arguments
/// with no references of generic lifetimes are supported without the `nightly` feature flag.
///
/// # Example
///
/// These examples require the `nightly` feature to be enabled. [`FnObject::call_object`] can be
/// used instead of direct calls on stable.
///
#[cfg_attr(feature = "nightly", doc = " ```")]
#[cfg_attr(not(feature = "nightly"), doc = " ```ignore")]
/// # use crossmist::fns::{FnObject, StaticFn};
/// fn add(a: i32, b: i32) -> i32 {
///     a + b
/// }
/// let add = unsafe { StaticFn::<fn(i32, i32) -> i32>::new(add) };
/// let add: Box<dyn FnObject<(i32, i32), Output = i32>> = Box::new(add);
/// assert_eq!(add(5, 7), 12);
/// ```
///
#[cfg_attr(feature = "nightly", doc = " ```")]
#[cfg_attr(not(feature = "nightly"), doc = " ```ignore")]
/// # use crossmist::fns::{FnObject, StaticFn};
/// let add = unsafe { StaticFn::<fn(i32, i32) -> i32>::new(|a, b| a + b) };
/// let add: Box<dyn FnObject<(i32, i32), Output = i32>> = Box::new(add);
/// assert_eq!(add(5, 7), 12);
/// ```
///
/// This example works on stable without changes.
///
/// ```rust
/// # use crossmist::fns::{FnObject, StaticFn};
/// unsafe fn dangerous_read(p: *const i32) -> i32 {
///     p.read()
/// }
/// let dangerous_read = unsafe { StaticFn::<unsafe fn(*const i32) -> i32>::new(dangerous_read) };
/// let dangerous_read = dangerous_read.get_fn();
/// unsafe {
///     assert_eq!(dangerous_read(&123), 123);
/// }
/// ```
///
/// This example requires `nightly` because of references.
///
#[cfg_attr(feature = "nightly", doc = " ```")]
#[cfg_attr(not(feature = "nightly"), doc = " ```ignore")]
/// # use crossmist::fns::{FnObject, StaticFn};
/// fn safe_read(p: &i32) -> i32 {
///     *p
/// }
/// let safe_read = unsafe { StaticFn::<fn(&i32) -> i32>::new(safe_read) };
/// let safe_read: Box<dyn FnObject<(&i32,), Output = i32>> = Box::new(safe_read);
/// assert_eq!(safe_read(&123), 123);
/// ```
#[derive(Clone, Copy, Debug, Object)]
pub struct StaticFn<F: FnPtr> {
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

macro_rules! impl_fn_pointer {
    () => {};
    ($head:tt $($tail:tt)*) => {
        paste! {
            #[cfg(not(feature = "nightly"))]
            impl<Output, $([<T $tail>]),*> fn_ptr_private::Sealed for fn($([<T $tail>]),*) -> Output {}
            #[cfg(not(feature = "nightly"))]
            impl<Output, $([<T $tail>]),*> FnPtr for fn($([<T $tail>]),*) -> Output {
                fn addr(self) -> usize {
                    self as usize
                }
            }

            #[cfg(not(feature = "nightly"))]
            impl<Output, $([<T $tail>]),*> fn_ptr_private::Sealed for unsafe fn($([<T $tail>]),*) -> Output {}
            #[cfg(not(feature = "nightly"))]
            impl<Output, $([<T $tail>]),*> FnPtr for unsafe fn($([<T $tail>]),*) -> Output {
                fn addr(self) -> usize {
                    self as usize
                }
            }

            impl_fn! {
                impl[T: FnPtr, Output, $([<T $tail>]),*] FnOnce<($([<T $tail>],)*), Output = Output> for StaticFn<T> where[T: FnOnce($([<T $tail>]),*) -> Output] =
                |self, args| {
                    let ($([<a $tail>],)*) = args;
                    self.get_fn()($([<a $tail>]),*)
                }
            }
            impl_fn! {
                impl[T: FnPtr, Output, $([<T $tail>]),*] FnMut<($([<T $tail>],)*)> for StaticFn<T> where[T: FnMut($([<T $tail>]),*) -> Output] =
                |self, args| {
                    let ($([<a $tail>],)*) = args;
                    self.get_fn()($([<a $tail>]),*)
                }
            }
            impl_fn! {
                impl[T: FnPtr, Output, $([<T $tail>]),*] Fn<($([<T $tail>],)*)> for StaticFn<T> where[T: Fn($([<T $tail>]),*) -> Output] =
                |self, args| {
                    let ($([<a $tail>],)*) = args;
                    self.get_fn()($([<a $tail>]),*)
                }
            }
        }

        impl_fn_pointer!($($tail)*);
    };
}
impl_fn_pointer!(x 20 19 18 17 16 15 14 13 12 11 10 9 8 7 6 5 4 3 2 1 0);
