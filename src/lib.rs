//! Efficient and seamless cross-process communication, providing semantics similar to
//! [`std::thread::spawn`] and alike, both synchronously and asynchronously (via tokio).
//!
//! This crate allows you to easily perform computations in another process without creating a
//! separate executable or parsing command line arguments manually. For example, the simplest
//! example, computing a sum of several numbers in a one-shot subprocess, looks like this:
//!
//! ```rust
//! #[multiprocessing::main]
//! fn main() {
//!     println!("5 + 7 = {}", add.run(vec![5, 7]).unwrap());
//! }
//!
//! #[multiprocessing::func]
//! fn add(nums: Vec<i32>) -> i32 {
//!     nums.into_iter().sum()
//! }
//! ```
//!
//! This crate also supports long-lived tasks with constant cross-process communication:
//!
//! ```rust
//! #[multiprocessing::main]
//! fn main() {
//!     let (mut ours, theirs) = multiprocessing::duplex().unwrap();
//!     add.spawn(theirs).expect("Failed to spawn child");
//!     for i in 1..=5 {
//!         for j in 1..=5 {
//!             println!("{i} + {j} = {}", ours.request(&vec![5, 7]).unwrap());
//!         }
//!     }
//! }
//!
//! #[multiprocessing::func]
//! fn add(mut chan: multiprocessing::Duplex<i32, Vec<i32>>) {
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
//! use multiprocessing::Object;
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
#![feature(unboxed_closures)]
#![feature(fn_traits)]
#![feature(ptr_metadata)]
#![feature(never_type)]
#![feature(try_blocks)]
#![feature(unwrap_infallible)]
#![feature(tuple_trait)]

extern crate self as multiprocessing;

pub use multiprocessing_derive::*;

#[doc(hidden)]
pub mod imp;

pub mod serde;
pub use crate::serde::*;

mod platform {
    #[cfg(unix)]
    pub mod unix {
        pub mod handles;
        pub mod ipc;
        pub mod subprocess;
        #[doc(cfg(feature = "tokio"))]
        #[cfg(feature = "tokio")]
        pub mod tokio;
    }
    #[cfg(windows)]
    pub mod windows {
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
