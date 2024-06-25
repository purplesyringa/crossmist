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
//! use crossmist::{func, main, Object};
//!
//! #[main]
//! fn main() {
//!     let x = 7;
//!     println!("{}", go.run(5, Box::new(|y| x + y)).unwrap());
//! }
//!
//! #[func]
//! fn go(x: i32, f: Box<dyn Object + Fn(i32) -> i32>) -> i32 {
//!     f(x)
//! }
//! ```
//!
//! ...we have to use a macro, and also a different invocation syntax:
//!
//! ```rust
//! use crossmist::{FnObject, func, lambda, main};
//!
//! #[main]
//! fn main() {
//!     let x = 7;
//!     println!("{}", go.run(5, lambda! { move(x: i32) |y: i32| -> i32 { x + y } }).unwrap());
//! }
//!
//! #[func]
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
//! ```rust
//! use crossmist::{FnObject, func, lambda, main};
//!
//! #[main]
//! fn main() {
//!     let x = Box::new(7);
//!     println!("{}", go.run(5, lambda! { move(&x: &Box<i32>) |y: i32| -> i32 { **x + y } }).unwrap());
//! }
//!
//! #[func]
//! fn go(x: i32, f: Box<dyn FnObject<(i32,), Output = i32>>) -> i32 {
//!     f.call_object((x,))
//! }
//! ```
//!
//! Similarly, `&mut x` can be used if the object is to be modified. Note that this still moves `x`
//! into the lambda.
//!
//! Under the hood, the macro uses currying, replacing `|y| x + y` with `|x, y| x + y` with a
//! pre-determined `x` variable, and makes `|x, y| x + y` a callable [`Object`] by using `#[func]`:
//!
//! ```rust
//! use crossmist::{BindValue, FnObject, func, main};
//!
//! #[main]
//! fn main() {
//!     #[func]
//!     fn add(x: i32, y: i32) -> i32 {
//!         x + y
//!     }
//!
//!     let x = 7;
//!     println!("{}", go.run(5, Box::new(add.bind_value(x))).unwrap());
//! }
//!
//! #[func]
//! fn go(x: i32, f: Box<dyn FnObject<(i32,), Output = i32>>) -> i32 {
//!     f.call_object((x,))
//! }
//! ```

use crate::{relocation::RelocatablePtr, Object};
use paste::paste;
use std::marker::PhantomData;
use std::ops::Deref;

macro_rules! impl_fn {
    (
        impl[$($generic_bounds:tt)*] FnOnce<$args_ty:ty, Output = $output:ty> for $target:ty =
        $(#[$attr:meta])*
        |$self:tt, $args:tt| {
            $($body:tt)*
        }
    ) => {
        #[cfg(feature = "nightly")]
        impl<$($generic_bounds)*> std::ops::FnOnce<$args_ty> for $target {
            type Output = $output;
            $(#[$attr])*
            #[allow(unused_mut)]
            extern "rust-call" fn call_once(mut $self, $args: $args_ty) -> Self::Output {
                $($body)*
            }
        }
        #[cfg(not(feature = "nightly"))]
        impl<$($generic_bounds)*> FnOnceObject<$args_ty> for $target {
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
        impl[$($generic_bounds:tt)*] FnMut<$args_ty:ty> for $target:ty =
        $(#[$attr:meta])*
        |$self:tt, $args:tt| {
            $($body:tt)*
        }
    ) => {
        #[cfg(feature = "nightly")]
        impl<$($generic_bounds)*> std::ops::FnMut<$args_ty> for $target {
            $(#[$attr])*
            extern "rust-call" fn call_mut(&mut $self, $args: $args_ty) -> Self::Output {
                $($body)*
            }
        }
        #[cfg(not(feature = "nightly"))]
        impl<$($generic_bounds)*> FnMutObject<$args_ty> for $target {
            $(#[$attr])*
            fn call_object_mut(&mut $self, $args: $args_ty) -> Self::Output {
                $($body)*
            }
        }
    };

    (
        impl[$($generic_bounds:tt)*] Fn<$args_ty:ty> for $target:ty =
        $(#[$attr:meta])*
        |$self:tt, $args:tt| {
            $($body:tt)*
        }
    ) => {
        #[cfg(feature = "nightly")]
        impl<$($generic_bounds)*> std::ops::Fn<$args_ty> for $target {
            $(#[$attr])*
            extern "rust-call" fn call(&$self, $args: $args_ty) -> Self::Output {
                $($body)*
            }
        }
        #[cfg(not(feature = "nightly"))]
        impl<$($generic_bounds)*> FnObject<$args_ty> for $target {
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
    ///     move(&mut counter: &mut i32) || -> i32 { *counter += 1; *counter }
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
    ///     move(&mut counter: &mut i32) || -> i32 { *counter += 1; *counter }
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
    /// use crossmist::{FnObject, func};
    ///
    /// #[func]
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
    /// use crossmist::{FnObject, func};
    ///
    /// #[func]
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

/// A short-cut for turning a (possible capturing) closure into an object function, just like as if
/// `#[func]` was used.
///
/// Syntax is similar to that of closure, except that types of all arguments and the type of the
/// return value are not inferred. Additionally, all moved values have to be listed manually,
/// indicating how they are captured.
///
/// Simplest example:
///
/// ```rust
/// # use crossmist::{lambda, main};
/// #[main]
/// fn main() {
///     let func = lambda! { |a: i32, b: i32| -> i32 { a + b } };
///     assert_eq!(func.run(5, 7).unwrap(), 12);
/// }
/// ```
///
/// With captures:
///
/// ```rust
/// # use crossmist::{FnObject, FnOnceObject, func, lambda, main};
/// #[main]
/// fn main() {
///     let a = 5;
///     let func = lambda! { move(a: i32) |b: i32| -> i32 { a + b } };
///     // run/spawn do not work directly, but you may still call/pass the function
///     assert_eq!(func.call_object((7,)), 12);
///     assert_eq!(gate.run(func, 7).unwrap(), 12);
/// }
///
/// #[func]
/// fn gate(f: Box<dyn FnOnceObject<(i32,), Output = i32>>, arg: i32) -> i32 {
///     f.call_object_once((arg,))
/// }
/// ```
///
/// `f.call_object_once((arg,))` can be replaced with `f(arg)` if the `nightly` feature is enabled.
///
/// Captuing more complex objects (type annotations are provided for completeness and are
/// unnecessary):
///
/// ```rust
/// # use crossmist::{FnOnceObject, lambda, main};
/// # #[main]
/// # fn main() {
/// let a = "Hello, ".to_string();
/// // a is accessible by value when the lambda is executed
/// let prepend_hello: Box<dyn FnOnceObject<(&str,), Output = String>> =
///     lambda! { move(a: String) |b: &str| -> String { a + b } };
/// assert_eq!(prepend_hello.call_object_once(("world!",)), "Hello, world!".to_string());
/// // Can only be called once. The line below fails to compile when uncommented:
/// // assert_eq!(prepend_hello.call_object_once(("world!",)), "Hello, world!".to_string());
/// # }
/// ```
///
/// ```rust
/// # use crossmist::{FnMutObject, lambda, main};
/// # #[main]
/// # fn main() {
/// let cache = vec![0, 1];
/// // cache is accessible by a mutable reference when the lambda is executed
/// let mut fibonacci: Box<dyn FnMutObject<(usize,), Output = u32>> = lambda! {
///     move(&mut cache: &mut Vec<u32>) |n: usize| -> u32 {
///         while cache.len() <= n {
///             cache.push(cache[cache.len() - 2..].iter().sum());
///         }
///         cache[n]
///     }
/// };
/// assert_eq!(fibonacci.call_object_mut((3,)), 2);
/// // Can be called multiple types, but has to be mutable
/// assert_eq!(fibonacci.call_object_mut((6,)), 8);
/// # }
/// ```
///
/// ```rust
/// # use crossmist::{FnObject, lambda, main};
/// # #[main]
/// # fn main() {
/// let s = "Hello, world!".to_string();
/// // s is accessible by an immutable reference when the lambda is executed
/// let count_occurrences: Box<dyn FnObject<(char,), Output = usize>> =
///     lambda! { move(&s: &String) |c: char| -> usize { s.matches(c).count() } };
/// assert_eq!(count_occurrences.call_object(('o',)), 2);
/// // Can be called multiple times and be immutable
/// assert_eq!(count_occurrences.call_object(('e',)), 1);
/// # }
/// ```
#[macro_export]
macro_rules! lambda {
    // split || into | |
    (|| $($items:tt)*) => {
        $crate::lambda_parse! {
            [],
            [_unnamed],
            | $($items)*
        }
    };
    (|$($items:tt)*) => {
        $crate::lambda_parse! {
            [],
            [_unnamed],
            $($items)*
        }
    };
    // split || into | |
    (move($($moved_vars:tt)*) || $($items:tt)*) => {
        $crate::lambda_parse! {
            [],
            [
                $crate::lambda_bind! { [_unnamed], $($moved_vars)* }
            ],
            $($moved_vars)*, | $($items)*
        }
    };
    (move($($moved_vars:tt)*) |$($items:tt)*) => {
        $crate::lambda_parse! {
            [],
            [
                $crate::lambda_bind! { [_unnamed], $($moved_vars)* }
            ],
            $($moved_vars)*, $($items)*
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! lambda_parse {
    (
        [$($args:tt)*],
        [$($append:tt)*],
        , $($rest:tt)*
    ) => {
        $crate::lambda_parse! { [$($args)*], [$($append)*], $($rest)* }
    };
    (
        [$($args:tt)*],
        [$($append:tt)*],
        &mut $name:ident: $type:ty, $($rest:tt)*
    ) => {
        $crate::lambda_parse! { [$($args)* $name: $type,], [$($append)*], $($rest)* }
    };
    (
        [$($args:tt)*],
        [$($append:tt)*],
        $(&)? $name:ident: $type:ty, $($rest:tt)*
    ) => {
        $crate::lambda_parse! { [$($args)* $name: $type,], [$($append)*], $($rest)* }
    };
    (
        [$($args:tt)*],
        [$($append:tt)*],
        &mut $name:ident: $type:ty| $($rest:tt)*
    ) => {
        $crate::lambda_parse! { [$($args)* $name: $type,], [$($append)*], |$($rest)* }
    };
    (
        [$($args:tt)*],
        [$($append:tt)*],
        $(&)? $name:ident: $type:ty| $($rest:tt)*
    ) => {
        $crate::lambda_parse! { [$($args)* $name: $type,], [$($append)*], |$($rest)* }
    };

    (
        [$($args:tt)*],
        [$($append:tt)*],
        | -> $return_type:ty { $($code:tt)* }
    ) => {
        {
            #[$crate::func]
            fn _unnamed($($args)*) -> $return_type {
                $($code)*
            }
            {
                #[allow(unused)]
                use $crate::{BindValue, BindMut, BindRef};
                ::std::boxed::Box::new($($append)*)
            }
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! lambda_bind {
    ([$($acc:tt)*],) => { $($acc)* };

    ([$($acc:tt)*], &mut $name:ident: $type:ty, $($rest:tt)*) => {
        $crate::lambda_bind! { [$($acc)*.bind_mut($name)], $($rest)* }
    };
    ([$($acc:tt)*], &mut $name:ident: $type:ty) => {
        $($acc)*.bind_mut($name)
    };
    ([$($acc:tt)*], &$name:ident: $type:ty, $($rest:tt)*) => {
        $crate::lambda_bind! { [$($acc)*.bind_ref($name)], $($rest)* }
    };
    ([$($acc:tt)*], &$name:ident: $type:ty) => {
        $($acc)*.bind_ref($name)
    };
    ([$($acc:tt)*], $name:ident: $type:ty, $($rest:tt)*) => {
        $crate::lambda_bind! { [$($acc)*.bind_value($name)], $($rest)* }
    };
    ([$($acc:tt)*], $name:ident: $type:ty) => {
        $($acc)*.bind_value($name)
    };
}

/// Metaprogramming on `fn(...) -> ...` types.
///
/// This trait is not part of the stable API provided by crossmist.
pub trait FnPtr {
    /// Function arguments as a tuple.
    type Args: Tuple;
    /// Function output type.
    type Output;
    /// Convert the function pointer to a type-erased pointer.
    fn addr(self) -> *const ();
}

/// A wrapper for `fn(...) -> ...` implementing `Object`.
///
/// This type enables you to pass `fn` and `unsafe fn` pointers between processes soundly without
/// requiring [`lambda`] or [`crossmist::func`].
///
/// Creating the wrapper from a function pointer is `unsafe` because functions might not be
/// available in the child process if they were created in runtime by JIT compilation or alike.
///
/// # Example
///
/// ```rust
/// # use crossmist::fns::{FnObject, StaticFn};
/// fn add(a: i32, b: i32) -> i32 {
///     a + b
/// }
/// let add = unsafe { StaticFn::<fn(i32, i32) -> i32>::new(add) };
/// let add: Box<dyn FnObject<(i32, i32), Output = i32>> = Box::new(add);
/// assert_eq!(add(5, 7), 12);
/// ```
///
/// ```rust
/// # use crossmist::fns::{FnObject, StaticFn};
/// let add = unsafe { StaticFn::<fn(i32, i32) -> i32>::new(|a, b| a + b) };
/// let add: Box<dyn FnObject<(i32, i32), Output = i32>> = Box::new(add);
/// assert_eq!(add(5, 7), 12);
/// ```
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
            ptr: RelocatablePtr(f.addr()),
            phantom: PhantomData,
        }
    }

    /// Extract a function pointer from a [`StaticFn`].
    pub fn get_fn(self) -> F {
        assert!(std::mem::size_of::<*const ()>() == std::mem::size_of::<F>());
        unsafe { std::mem::transmute_copy::<*const (), F>(&self.ptr.0) }
    }
}

macro_rules! impl_fn_pointer {
    () => {};
    ($head:tt $($tail:tt)*) => {
        paste! {
            impl<Output, $([<T $tail>]),*> FnPtr for fn($([<T $tail>]),*) -> Output {
                type Args = ($([<T $tail>],)*);
                type Output = Output;
                fn addr(self) -> *const () {
                    self as *const ()
                }
            }
            impl_fn! {
                impl[Output, $([<T $tail>]),*] FnOnce<($([<T $tail>],)*), Output = Output> for StaticFn<fn($([<T $tail>]),*) -> Output> =
                |self, args| {
                    let ($([<a $tail>],)*) = args;
                    self.get_fn()($([<a $tail>]),*)
                }
            }
            impl_fn! {
                impl[Output, $([<T $tail>]),*] FnMut<($([<T $tail>],)*)> for StaticFn<fn($([<T $tail>]),*) -> Output> =
                |self, args| {
                    self.call_object_once(args)
                }
            }
            impl_fn! {
                impl[Output, $([<T $tail>]),*] Fn<($([<T $tail>],)*)> for StaticFn<fn($([<T $tail>]),*) -> Output> =
                |self, args| {
                    self.call_object_once(args)
                }
            }

            impl<Output, $([<T $tail>]),*> fn_ptr_private::Sealed for unsafe fn($([<T $tail>]),*) -> Output {}
            impl<Output, $([<T $tail>]),*> FnPtr for unsafe fn($([<T $tail>]),*) -> Output {
                type Args = ($([<T $tail>],)*);
                type Output = Output;
                fn addr(self) -> *const () {
                    self as *const ()
                }
            }
        }

        impl_fn_pointer!($($tail)*);
    };
}
impl_fn_pointer!(x 20 19 18 17 16 15 14 13 12 11 10 9 8 7 6 5 4 3 2 1 0);
