//! Efficient and seamless cross-process communication, providing semantics similar to
//! [`std::thread::spawn`] and alike, both synchronously and asynchronously (via tokio).
//!
//! This crate allows you to easily perform computations in another process without creating a
//! separate executable or parsing command line arguments manually. For example, the simplest
//! example, computing a sum of several numbers in a one-shot subprocess, looks like this:
//!
//! ```rust
//! #[crossmist::main]
//! fn main() {
//!     println!("5 + 7 = {}", add.run(vec![5, 7]).unwrap());
//! }
//!
//! #[crossmist::func]
//! fn add(nums: Vec<i32>) -> i32 {
//!     nums.into_iter().sum()
//! }
//! ```
//!
//! This crate also supports long-lived tasks with constant cross-process communication:
//!
//! ```rust
//! #[crossmist::main]
//! fn main() {
//!     let (mut ours, theirs) = crossmist::duplex().unwrap();
//!     add.spawn(theirs).expect("Failed to spawn child");
//!     for i in 1..=5 {
//!         for j in 1..=5 {
//!             println!("{i} + {j} = {}", ours.request(&vec![i, j]).unwrap());
//!         }
//!     }
//! }
//!
//! #[crossmist::func]
//! fn add(mut chan: crossmist::Duplex<i32, Vec<i32>>) {
//!     while let Some(nums) = chan.recv().unwrap() {
//!         chan.send(&nums.into_iter().sum());
//!     }
//! }
//! ```
//!
//!
//! ## Passing objects
//!
//! Almost arbitrary objects can be passed between processes and across channels, including file
//! handles, sockets, and other channels.
//!
//! For numeric types, strings, vectors, hashmaps, other common containers, and files/sockets, the
//! [`Object`] trait is implemented automatically. For user-defined structures and enums, use
//! `#[derive(Object)]`. You may use generics, but make sure to add `: Object` constraint to stored
//! types:
//!
//! ```rust
//! use crossmist::Object;
//!
//! #[derive(Object)]
//! struct MyPair<T: Object, U: Object> {
//!     first: T,
//!     second: U,
//! }
//! ```
//!
//! Occasionally, e.g. for custom hash tables or externally defined types, you might have to
//! implement [`Object`] manually. Check out the documentation for [`Object`] for more information.
//!
//!
//! ## Channels
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

#![cfg_attr(unix, feature(unix_socket_ancillary_data))]
#![feature(doc_cfg)]
#![feature(fn_traits)]
#![feature(never_type)]
#![feature(ptr_metadata)]
#![feature(specialization)]
#![feature(try_blocks)]
#![feature(tuple_trait)]
#![feature(unboxed_closures)]
#![feature(unwrap_infallible)]

extern crate self as crossmist;

/// Enable a function to be used as an entrypoint of a child process, and turn it into an
/// [`Object`].
///
/// This macro applies to `fn` functions, including generic ones. It turns the function into an
/// object that can be called (providing the same behavior as if `#[func]` was not used), but also
/// adds various methods for spawning a child process from this function.
///
/// For a function declared as
///
/// ```ignore
/// #[func]
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
/// ```rust
/// use crossmist::{func, main};
///
/// #[func]
/// fn example(a: i32, b: i32) -> i32 {
///     a + b
/// }
///
/// #[main]
/// fn main() {
///     assert_eq!(example(5, 7), 12);
///     assert_eq!(example.spawn(5, 7).unwrap().join().unwrap(), 12);
///     assert_eq!(example.run(5, 7).unwrap(), 12);
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
/// ```rust
/// #[crossmist::main]
/// fn main() {
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
/// #[crossmist::func]
/// fn long_running_task() -> u32 {
///     std::thread::sleep(std::time::Duration::from_secs(1));
///     123
/// }
/// ```
///
/// Don't:
///
/// ```no_run
/// #[crossmist::main]
/// fn main() {
///     let child = long_running_task.spawn().expect("Failed to spawn child");
///     // ...
///     let need_child_result = false;  // assume this is computed from some external data
///     // ...
///     if need_child_result {
///         eprintln!("{}", child.join().expect("Child died"));
///     }
/// }
///
/// #[crossmist::func]
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
/// ```rust
/// #[crossmist::main]
/// fn main() {
///     long_running_task.spawn().expect("Failed to spawn child");
/// }
///
/// #[crossmist::func]
/// fn long_running_task() {
///     std::thread::sleep(std::time::Duration::from_secs(1));
/// }
/// ```
///
/// Do:
///
/// ```rust
/// #[crossmist::main]
/// fn main() {
///     let child = long_running_task.spawn().expect("Failed to spawn child");
///     // ...
///     child.join().expect("Child died");
/// }
///
/// #[crossmist::func]
/// fn long_running_task() {
///     std::thread::sleep(std::time::Duration::from_secs(1));
///     std::process::exit(0);
/// }
/// ```
///
///
/// ## Asynchronous case
///
/// This section applies if the `tokio` feature is enabled.
///
/// The following methods are also made available:
///
/// ```ignore
/// pub async fn spawn_tokio(&self, arg1: Type1, ...) ->
///     std::io::Result<crossmist::tokio::Child<Output>>;
/// pub async fn run_tokio(&self, arg1: Type1, ...) -> std::io::Result<Output>;
/// ```
///
/// Additionally, the function may be `async`. In this case, you have to add the `#[tokio::main]`
/// attribute *after* `#[func]`. For instance:
///
/// ```ignore
/// #[crossmist::func]
/// #[tokio::main]
/// async fn example() {}
/// ```
///
/// You may pass operands to `tokio::main` just like usual:
///
/// ```rust
/// #[crossmist::func]
/// #[tokio::main(flavor = "current_thread")]
/// async fn example() {}
/// ```
///
/// Notice that the use of `spawn` vs `spawn_tokio` is orthogonal to whether the function is
/// `async`: you can start a synchronous function in a child process asynchronously, or vice versa:
///
/// ```rust
/// use crossmist::{func, main};
///
/// #[func]
/// fn example(a: i32, b: i32) -> i32 {
///     a + b
/// }
///
/// #[main]
/// #[tokio::main(flavor = "current_thread")]
/// async fn main() {
///     assert_eq!(example(5, 7), 12);
///     assert_eq!(example.run_tokio(5, 7).await.unwrap(), 12);
/// }
/// ```
///
/// ```rust
/// use crossmist::{func, main};
///
/// #[func]
/// #[tokio::main(flavor = "current_thread")]
/// async fn example(a: i32, b: i32) -> i32 {
///     a + b
/// }
///
/// #[main]
/// fn main() {
///     assert_eq!(example.run(5, 7).unwrap(), 12);
/// }
/// ```
pub use crossmist_derive::func;

/// Setup an entrypoint.
///
/// This attribute must always be added to `fn main`:
///
/// ```rust
/// #[crossmist::main]
/// fn main() {
///     // ...
/// }
/// ```
///
/// Without it, starting child processes will panic.
///
/// This attribute may be mixed with other attributes, e.g. `#[tokio::main]`. In this case, this
/// attribute should be the first in the list:
///
/// ```rust
/// #[crossmist::main]
/// #[tokio::main(flavor = "current_thread")]
/// async fn main() {
///     // ...
/// }
/// ```
pub use crossmist_derive::main;

/// Make a structure or a enum serializable.
///
/// This derive macro enables the corresponding type to be passed via channels and to and from child
/// processes. [`Object`] can be implemented for a struct/enum if all of its fields implement
/// [`Object`]:
///
/// This is okay:
///
/// ```rust
/// # use crossmist::Object;
/// #[derive(Object)]
/// struct Test(String, i32);
/// ```
///
/// This is not okay:
///
/// ```compile_fail
/// # use crossmist::Object;
/// struct NotObject;
///
/// #[derive(Object)]
/// struct Test(String, i32, NotObject);
/// ```
///
/// Generics are supported. In this case, to ensure that all fields implement [`Object`],
/// constraints might be necessary:
///
/// This is okay:
///
/// ```rust
/// # use crossmist::Object;
/// #[derive(Object)]
/// struct MyPair<T: Object>(T, T);
/// ```
///
/// This is not okay:
///
/// ```compile_fail
/// # use crossmist::Object;
/// #[derive(Object)]
/// struct MyPair<T>(T, T);
/// ```
pub use crossmist_derive::Object;

#[doc(hidden)]
pub mod imp;

pub mod serde;
pub use crate::serde::*;

mod platform {
    #[cfg(unix)]
    pub mod unix {
        pub(crate) mod entry;
        pub mod handles;
        pub mod ipc;
        pub mod subprocess;
        #[doc(cfg(feature = "tokio"))]
        #[cfg(feature = "tokio")]
        pub mod tokio;
    }
    #[cfg(windows)]
    pub mod windows {
        pub(crate) mod entry;
        pub mod handles;
        pub mod ipc;
        pub mod subprocess;
        #[doc(cfg(feature = "tokio"))]
        #[cfg(feature = "tokio")]
        pub mod tokio;
    }
}

#[cfg(unix)]
pub use crate::platform::unix::*;
#[cfg(windows)]
pub use crate::platform::windows::*;

pub use ipc::{channel, duplex, Duplex, Receiver, Sender};

pub use subprocess::*;

mod builtins;

pub mod delayed;
pub use delayed::Delayed;

pub mod fns;
pub use fns::*;

mod pod;
pub use pod::Object;
