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
//! Almost arbitrary objects can be passed between processes and across channels, including file
//! handles, sockets, and other channels.

#![cfg_attr(unix, feature(unix_socket_ancillary_data))]
#![feature(unboxed_closures)]
#![feature(fn_traits)]
#![feature(ptr_metadata)]
#![feature(never_type)]
#![feature(try_blocks)]
#![feature(unwrap_infallible)]
#![feature(tuple_trait)]

extern crate self as multiprocessing;

pub use multiprocessing_derive::*;

pub mod imp;

pub mod serde;
pub use crate::serde::*;

mod platform {
    #[cfg(unix)]
    pub mod unix {
        pub mod handles;
        pub mod ipc;
        pub mod subprocess;
        #[cfg(feature = "tokio")]
        pub mod tokio;
    }
    #[cfg(windows)]
    pub mod windows {
        pub mod handles;
        pub mod ipc;
        pub mod subprocess;
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

pub mod builtins;

pub mod delayed;
pub use delayed::Delayed;

pub mod fns;
pub use fns::*;
