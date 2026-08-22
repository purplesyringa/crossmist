//! crossmist implements of [`std::thread::spawn`] for processes and channels for cross-process
//! communication.
//!
//! This allows you to easily move computations to another process, insulating you from the OOM
//! killer and global mutexes locks and allowing processes running in different environments (e.g.
//! [pidns](https://man7.org/linux/man-pages/man7/pid_namespaces.7.html)) to coordinate.
//!
//!
//! # Example
//!
//! Here's a simple example computing a sum of several numbers in a one-shot subprocess:
//!
//! ```standalone_crate
//! fn main() {
//!     // Let crossmist redirect control flow to other entrypoints
//!     crossmist::init();
//!     // Run `add` in a separate process and wait for it to finish
//!     println!("5 + 7 = {}", add.run(vec![5, 7]).unwrap());
//! }
//!
//! // Mark the function as a valid entrypoint
//! #[crossmist::entrypoint]
//! fn add(nums: Vec<i32>) -> i32 {
//!     nums.into_iter().sum()
//! }
//! ```
//!
//! Here's an example demonstrating using channels to coordinate long-living tasks:
//!
//! ```standalone_crate
//! fn main() {
//!     crossmist::init();
//!     // Create a bidirectional channel and spawn `add` in a separate process, passing one end of
//!     // the channel as an argument
//!     let (mut ours, theirs) = crossmist::duplex().unwrap();
//!     add.spawn(theirs).expect("Failed to spawn child");
//!     for i in 1..=5 {
//!         for j in 1..=5 {
//!             // Send the input and wait for a response
//!             println!("{i} + {j} = {}", ours.request(vec![i, j]).unwrap());
//!         }
//!     }
//! }
//!
//! #[crossmist::entrypoint]
//! fn add(mut chan: crossmist::Duplex<i32, Vec<i32>>) {
//!     while let Some(nums) = chan.recv().unwrap() {
//!         chan.send(nums.into_iter().sum());
//!     }
//! }
//! ```
//!
//!
//! # Nouns
//!
//! The two terms crossmist introduces are *objects* and *entrypoints*.
//!
//! Objects are values that can be passed between processes, either when starting a new process or
//! over a channel. Since processes don't share memory, objects have to be serialized when sent.
//! This is covered by the [`Object`] trait. Its implementors include:
//! - Most vocabulary types, like [`i32`], [`String`], and [`HashMap`](std::collections::HashMap).
//! - Compound types annotated with [`#[derive(Object)]`](derive@Object).
//! - File descriptors: you can send [`File`](std::fs::File) and [`TcpStream`](std::net::TcpStream)
//!   over a channel, or even a channel over another channel.
//! - Callbacks (closures) built with the [`func`] macro.
//! - `Box<dyn Trait>` when [`Object`] is a supertrait of `Trait`.
//!
//! Entrypoints are .
//!
//!
//! # Channels
//!
//! As the second example demonstrates, cross-process communication may be achieved not only via
//! arguments and return values, but via long-lived channels. Channels may be unidirectional (one
//! process has a [`Sender`] instance and another process has a connected [`Receiver`] instance) or
//! bidirectional (both processes have [`Duplex`] instances). Channels are typed: you don't just
//! send byte streams à la TCP, you send objects of a well-defined type implementing the [`Object`]
//! trait, making channels type-safe.
//!
//! Channels implement [`Object`]. This means that not only can you pass channels to subprocesses
//! as arguments (they wouldn't be useful otherwise), but you can pass channels across other
//! channels, just like you can pass files across channels.
//!
//! Channels are trusted. This means that if one side reads from [`Receiver`] and another side
//! writes garbage to the corresponding file descriptor instead of using [`Sender`], the receiver
//! side may crash and burn, potentially leading to arbitrary code execution.
//!
//! The communication protocol is not fixed and may not only change in minor versions, but be
//! architecture- or build-dependent. This is done to both ensure performance optimizations can be
//! implemented and to let us fix bugs quickly when they arise. As channels may only be used between
//! two processes started from the same executable file, this does not violate semver.
//!
//!
//! # Aborting computations
//!
//! If, at any point, you determine that you are no longer interested in the output of a process,
//! you can kill it:
//!
//! ```standalone_crate
//! fn main() {
//!     crossmist::init();
//!     let mut child = long_computation.spawn().expect("Failed to spawn child");
//!     let kill_handle = child.get_kill_handle();
//!     std::thread::spawn(move || {
//!         // Wait, I don't need this, actually!
//!         std::thread::sleep(std::time::Duration::from_millis(500));
//!         kill_handle.kill();
//!     });
//!     // This will fail in 0.5 seconds:
//!     child.join().unwrap_err();
//! }
//!
//! #[crossmist::entrypoint]
//! fn long_computation() {
//!     loop {}
//! }
//! ```
//!
//!
//! # Features
//!
//! This crate provides the following features:
//! - `tokio`: enable [Tokio](https://tokio.rs) async runtime support.
//! - `smol`: enable [smol](https://crates.io/crates/smol) async runtime support.
//! - `nightly`: make use of nightly features. This enables crossmist to be more performant and
//!   provide better API, but requires a nightly compiler to be used.

#![cfg_attr(
    feature = "nightly",
    feature(
        arbitrary_self_types_pointers,
        doc_cfg,
        fn_static,
        fn_traits,
        never_type,
        tuple_trait,
        unboxed_closures,
    )
)]
#![cfg_attr(all(doc, feature = "nightly"), feature(rustdoc_internals))]
#![cfg_attr(all(doc, feature = "nightly"), allow(internal_features))]
#![deny(missing_debug_implementations)]

extern crate self as crossmist;

/// Enable a function to be used as an entrypoint of a child process, and turn it into an
/// [`Object`].
///
/// This macro applies to `fn` functions, including generic ones. It adds various methods for
/// spawning a child process from this function.
///
/// For a function declared as
///
/// ```ignore
/// #[crossmist::entrypoint]
/// fn example(arg1: Type1, ...) -> Output;
/// ```
///
/// ...the methods are:
///
/// ```ignore
/// pub fn spawn(&self, arg1: Type1, ...) -> std::io::Result<crossmist::Child<Output>>;
/// pub fn run(&self, arg1: Type1, ...) -> std::io::Result<Output>;
/// ```
///
/// `spawn` runs the function in a subprocess and returns a [`Child`] instance which can be used to
/// monitor the process and retrieve its return value when it finishes via [`Child::join`]. `run`
/// combines the two operations into one, which may be useful if a new process is needed for a
/// reason other than parallel execution.
///
/// For example:
///
/// ```standalone_crate
/// #[crossmist::entrypoint]
/// fn example(a: i32, b: i32) -> i32 {
///     a + b
/// }
///
/// fn main() {
///     crossmist::init();
///     assert_eq!(example.spawn(5, 7).unwrap().join().unwrap(), 12);
///     assert_eq!(example.run(5, 7).unwrap(), 12);
/// }
/// ```
///
/// The function can also be invoked in *the same* process via the [`FnOnceObject`],
/// [`FnMutObject`], and [`FnObject`] traits, which are similar to [`std::ops::FnOnce`],
/// [`std::ops::FnMut`], and [`std::ops::Fn`], respectively:
///
/// ```standalone_crate
/// use crossmist::FnObject;
///
/// fn main() {
///     crossmist::init();
///     let example = crossmist::func! { |a, b| a + b };
///     assert_eq!(example.call_object((5, 7)), 12);
/// }
/// ```
///
/// If the `nightly` feature is enabled, the function can also directly be called, providing the
/// same behavior as if `#[crossmist::entrypoint]` was not used:
///
/// ```ignore
/// use crossmist::FnObject;
///
/// #[crossmist::entrypoint]
/// fn example(a: i32, b: i32) -> i32 {
///     a + b
/// }
///
/// fn main() {
///     crossmist::init();
///     assert_eq!(example(5, 7), 12);
/// }
/// ```
///
/// `spawn` and `run` return an error if spawning the child process failed (e.g. the process limit
/// is exceeded or the system lacks memory). `run` also returns an error if the process panics,
/// calls [`std::process::exit`] or alike instead of returning a value, or is terminated (as does
/// [`Child::join`]).
///
/// The child process relays its return value to the parent via an implicit channel. Therefore, it
/// is important to keep the [`Child`] instance around until the child process terminates and never
/// drop it before joining, or the child process will panic.
///
/// Do:
///
/// ```standalone_crate
/// fn main() {
///     crossmist::init();
///     let child = long_running_task.spawn().expect("Failed to spawn child");
///     // ...
///     let need_child_result = false;  // assume this is computed from some external data
///     // ...
///     let return_value = child.join().expect("Child died");
///     if need_child_result {
///         eprintln!("{return_value}");
///     }
/// }
///
/// #[crossmist::entrypoint]
/// fn long_running_task() -> u32 {
///     std::thread::sleep(std::time::Duration::from_secs(1));
///     123
/// }
/// ```
///
/// Don't:
///
/// ```no_run
/// fn main() {
///     crossmist::init();
///     let child = long_running_task.spawn().expect("Failed to spawn child");
///     // ...
///     let need_child_result = false;  // assume this is computed from some external data
///     // ...
///     if need_child_result {
///         eprintln!("{}", child.join().expect("Child died"));
///     }
/// }
///
/// #[crossmist::entrypoint]
/// fn long_running_task() -> u32 {
///     std::thread::sleep(std::time::Duration::from_secs(1));
///     123
/// }
/// ```
///
/// The void return type (`()`) is an exception to this rule: such return values are not delivered,
/// and thus [`Child`] may be safely dropped at any point, and the child process is allowed to use
/// [`std::process::exit`] instead of explicitly returning `()`.
///
/// Do:
///
/// ```standalone_crate
/// fn main() {
///     crossmist::init();
///     long_running_task.spawn().expect("Failed to spawn child");
/// }
///
/// #[crossmist::entrypoint]
/// fn long_running_task() {
///     std::thread::sleep(std::time::Duration::from_secs(1));
/// }
/// ```
///
/// Do:
///
/// ```standalone_crate
/// fn main() {
///     crossmist::init();
///     let child = long_running_task.spawn().expect("Failed to spawn child");
///     // ...
///     child.join().expect("Child died");
/// }
///
/// #[crossmist::entrypoint]
/// fn long_running_task() {
///     std::thread::sleep(std::time::Duration::from_secs(1));
///     std::process::exit(0);
/// }
/// ```
///
///
/// ## Asynchronous case
///
/// If the `tokio` feature is enabled, the following methods are also made available:
///
/// ```ignore
/// pub async fn spawn_tokio(&self, arg1: Type1, ...) ->
///     std::io::Result<crossmist::tokio::Child<Output>>;
/// pub async fn run_tokio(&self, arg1: Type1, ...) -> std::io::Result<Output>;
/// ```
///
/// If `smol` is enabled, the functions `spawn_smol` and `run_smol` with matching signatures are
/// generated.
///
/// Additionally, the function may be `async`. In this case, you have to indicate which runtime to
/// use as follows:
///
/// ```ignore
/// #[crossmist::entrypoint(tokio)]
/// async fn example_tokio() {}
///
/// #[crossmist::entrypoint(smol)]
/// async fn example_smol() {}
/// ```
///
/// With this syntax, the arguments to the functions are deserialized after the async runtime is
/// initialized. Simply using `#[crossmist::entrypoint]` followed by `#[tokio::main]` would deserialize
/// arguments before the runtime is started, leading to errors when deserializing channels.
///
/// You may pass operands to forward to `tokio::main` like this:
///
/// ```rust
/// #[crossmist::entrypoint(tokio(flavor = "current_thread"))]
/// async fn example() {}
/// ```
///
/// Notice that the use of `spawn` vs `spawn_tokio`/`spawn_smol` is orthogonal to whether the
/// function is `async`: you can start a synchronous function in a child process asynchronously, or
/// vice versa:
///
/// ```standalone_crate
/// #[crossmist::entrypoint]
/// fn example(a: i32, b: i32) -> i32 {
///     a + b
/// }
///
/// fn main() {
///     crossmist::init();
///     async_main();
/// }
///
/// #[tokio::main(flavor = "current_thread")]
/// async fn async_main() {
///     assert_eq!(example.run_tokio(5, 7).await.unwrap(), 12);
/// }
/// ```
///
/// ```standalone_crate
/// #[crossmist::entrypoint(tokio(flavor = "current_thread"))]
/// async fn example(a: i32, b: i32) -> i32 {
///     a + b
/// }
///
/// fn main() {
///     crossmist::init();
///     assert_eq!(example.run(5, 7).unwrap(), 12);
/// }
/// ```
pub use crossmist_derive::entrypoint;

/// A short-cut for turning a (possible capturing) closure into an object function.
///
/// Syntax is similar to that of closure, except that types of all arguments and the type of the
/// return value are not inferred. Additionally, all moved values have to be listed manually,
/// indicating how they are captured.
///
/// Simplest example:
///
/// ```standalone_crate
/// # use crossmist::{FnObject, FnOnceObject, func};
/// fn main() {
///     crossmist::init();
///     let func = func! { |a, b| a + b };
///     // run/spawn do not work directly, but you may still call/pass the function
///     assert_eq!(func.call_object((5, 7)), 12);
///     assert_eq!(gate.run(func).unwrap(), 12);
/// }
///
/// #[crossmist::entrypoint]
/// fn gate(f: Box<dyn FnOnceObject<(i32, i32), Output = i32>>) -> i32 {
///     f.call_object_once((5, 7))
/// }
/// ```
///
/// With captures:
///
/// ```standalone_crate
/// # use crossmist::{FnObject, FnOnceObject, func};
/// fn main() {
///     crossmist::init();
///     let a = 5;
///     let func = func! { move(a) |b| a + b };
///     assert_eq!(func.call_object_once((7,)), 12);
/// }
/// ```
///
/// `f.call_object_once((arg,))` can be replaced with `f(arg)` if the `nightly` feature is enabled.
///
/// Captuing more complex objects (type annotations are provided for completeness and are
/// unnecessary):
///
/// ```standalone_crate
/// # use crossmist::{FnOnceObject, func};
/// # fn main() {
/// # crossmist::init();
/// let a = "Hello, ".to_string();
/// // a is accessible by value when the func is executed
/// let prepend_hello: Box<dyn FnOnceObject<(&str,), Output = String>> =
///     func! { move(a) |b| a + b };
/// assert_eq!(prepend_hello.call_object_once(("world!",)), "Hello, world!".to_string());
/// // Can only be called once. The line below fails to compile when uncommented:
/// // assert_eq!(prepend_hello.call_object_once(("world!",)), "Hello, world!".to_string());
/// # }
/// ```
///
/// ```standalone_crate
/// # use crossmist::{FnMutObject, func};
/// # fn main() {
/// # crossmist::init();
/// let cache = vec![0, 1];
/// // cache is accessible by a mutable reference when the func is executed
/// let mut fibonacci: Box<dyn FnMutObject<(usize,), Output = u32>> = func! {
///     move(ref mut cache) |n| {
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
/// ```standalone_crate
/// # use crossmist::{FnObject, func};
/// # fn main() {
/// # crossmist::init();
/// let s = "Hello, world!".to_string();
/// // s is accessible by an immutable reference when the func is executed
/// let count_occurrences: Box<dyn FnObject<(char,), Output = usize>> =
///     func! { move(ref s) |c| s.matches(c).count() };
/// assert_eq!(count_occurrences.call_object(('o',)), 2);
/// // Can be called multiple times and be immutable
/// assert_eq!(count_occurrences.call_object(('e',)), 1);
/// # }
/// ```
pub use crossmist_derive::func;

/// Enable a `struct` or an `enum` to be sent across processes.
///
/// [`Object`] can be implemented if all fields of the `struct`/`enum` implement [`Object`]. For
/// generic definitions, [`Object`] bounds are automatically added for all generic parameters, so
/// you don't need to add them on the type itself:
///
/// ```rust
/// # use crossmist::Object;
/// #[derive(Object)]
/// struct MyPair<T>(T, T);
/// ```
///
/// This generates:
///
/// ```ignore
/// impl<T: Object> Object for MyPair<T> { ... }
/// ```
///
/// In case the automatically generated bounds are incorrect (e.g. if `T` is actually only stored in
/// [`PhantomData`](core::marker::PhantomData)), the `#[crossmist(bound = "...")]` attribute can be
/// used:
///
/// ```rust
/// # use crossmist::Object;
/// # use core::marker::PhantomData;
/// #[derive(Object)]
/// #[crossmist(bound = "U: Object")] // don't add T: Object
/// struct Partial<T, U>(PhantomData<T>, U);
/// ```
///
/// An empty `bound` parameter can be used to generate an [`Object`] implementation unconditionally.
pub use crossmist_derive::Object;

#[doc(hidden)]
pub mod imp;
pub use imp::init;

mod serde;
pub use serde::*;

mod owning_ref;

mod platform {
    #[cfg_attr(feature = "nightly", doc(cfg(all())))]
    #[cfg(unix)]
    pub mod unix {
        pub(crate) mod entry;
        pub(crate) mod internals;
        pub(crate) mod subprocess;
    }
    #[cfg(windows)]
    pub mod windows {
        pub(crate) mod entry;
        pub(crate) mod internals;
        pub(crate) mod subprocess;
    }
}

#[cfg(unix)]
pub(crate) use crate::platform::unix::*;
#[cfg(windows)]
pub(crate) use crate::platform::windows::*;

pub mod asynchronous;
pub mod blocking;
#[cfg(feature = "smol")]
pub mod smol;
#[cfg(feature = "tokio")]
pub mod tokio;

#[doc(inline)]
pub use asynchronous::KillHandle;
pub use blocking::{Child, Duplex, Receiver, Sender, channel, duplex};

pub(crate) mod relocation;

mod builtins;
mod unsized_builtins;

pub mod fns;
pub use fns::*;

mod static_ref;
pub use static_ref::StaticRef;
