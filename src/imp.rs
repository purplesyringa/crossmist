#[cfg(feature = "smol")]
pub use async_io;

use std::sync::atomic::{AtomicBool, Ordering};

static INITIALIZED: AtomicBool = AtomicBool::new(false);

pub(crate) fn perform_sanity_checks() {
    assert!(
        INITIALIZED.load(Ordering::Relaxed),
        "crossmist::init() wasn't called"
    );
}

/// Initialize the crossmist runtime.
///
/// This function should always be called at the beginning of `main`.
///
/// When crossmist spawns child processes, they start executing the same `main` function as the root
/// process. Calling [`init`] lets crossmist pass control to the entrypoint that the process is
/// actually supposed to be executing.
///
/// [`init`] should not be invoked before `main`, e.g. with crates like `ctor`, since it assumes
/// `std` has been fully initialized. Attempting to do so may result in anything from misbehaving
/// user code to recursively re-executing the same program instead of running an entrypoint.
/// In particular, using `crossmist` in tests requires
/// [a custom harness](https://www.unwoundstack.com/blog/integration-testing-rust-binaries.html)
/// with a global setup hook calling [`init`].
///
/// # Async
///
/// In asynchronous programs, avoid annotating `main` with `#[tokio::main]` directly, and prefer:
///
/// ```rust
/// fn main() {
///     crossmist::init();
///     async_main();
/// }
///
/// #[tokio::main(flavor = "current_thread")]
/// async fn async_main() {
///     // ...
/// }
/// ```
///
/// Otherwise the async runtime can start unnecessarily if the entrypoint is synchronous, or it can
/// start twice if it's asynchronous.
pub fn init() {
    if INITIALIZED.swap(true, Ordering::Relaxed) {
        panic!("crossmist::init() is called twice");
    }

    let mut args = std::env::args();
    if args.next().as_deref() == Some("_crossmist_") {
        crate::entry::crossmist_main(args);
    }

    #[cfg(windows)]
    crate::subprocess::start_broker().expect("failed to start broker");
}

#[cfg(feature = "tokio")]
#[doc(hidden)]
#[macro_export]
macro_rules! if_tokio {
    ($($a:tt)*) => { $($a)* };
}
#[cfg(not(feature = "tokio"))]
#[doc(hidden)]
#[macro_export]
macro_rules! if_tokio {
    ($($a:tt)*) => {};
}

#[cfg(feature = "smol")]
#[doc(hidden)]
#[macro_export]
macro_rules! if_smol {
    ($($a:tt)*) => { $($a)* };
}
#[cfg(not(feature = "smol"))]
#[doc(hidden)]
#[macro_export]
macro_rules! if_smol {
    ($($a:tt)*) => {};
}
