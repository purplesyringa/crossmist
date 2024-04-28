pub use crate::pod::PlainOldData;

use crate::entry;
use std::sync::atomic::{AtomicBool, Ordering};

pub static INITIALIZED: AtomicBool = AtomicBool::new(false);

pub(crate) fn perform_sanity_checks() {
    assert!(
        INITIALIZED.load(Ordering::Acquire),
        "#[crossmist::main] or a call to crossmist::init() is missing"
    );
}

pub trait Report {
    fn report(self) -> i32;
}

impl Report for () {
    fn report(self) -> i32 {
        0
    }
}

impl<T, E: std::fmt::Debug> Report for Result<T, E> {
    fn report(self) -> i32 {
        match self {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("Error: {e:?}");
                1
            }
        }
    }
}

// We use this little trick to implement the 'trivial_bounds' feature in stable Rust. Instead of
// 'where T: Bounds', we use 'where for<'a> Identity<'a, T>: Bounds'. This seems to confuse the
// hell out of rustc and makes it believe the where clause is not trivial. Credits go to
// @danielhenrymantilla at GitHub, see:
// - https://github.com/getditto/safer_ffi/blob/65a8a2d8ccfd5ef5b5f58a495bc8cea9da07c6fc/src/_lib.rs#L519-L534
// - https://github.com/getditto/safer_ffi/blob/64b921bdcabe441b957742332773248af6677a89/src/proc_macro/utils/trait_impl_shenanigans.rs#L6-L28
pub type Identity<'a, T> = <T as IdentityImpl<'a>>::Type;
pub trait IdentityImpl<'a> {
    type Type: ?Sized;
}
impl<T: ?Sized> IdentityImpl<'_> for T {
    type Type = Self;
}

trait IfVoid {
    fn if_void() -> Option<Self>
    where
        Self: Sized;
}
impl<T> IfVoid for T {
    default fn if_void() -> Option<Self> {
        None
    }
}
impl IfVoid for () {
    fn if_void() -> Option<Self> {
        Some(())
    }
}

/// Returns Some(()) if T is (), None otherwise
///
/// This function is used to enable simplistic overloading for generic types with the ability to
/// hard-code simpler behavior for () than for other types while being able to construct () without
/// needing to prove T = () at the moment of construction.
///
/// At the moment, this is used to avoid explicitly sending () to the parent on child completion.
/// This is explicitly pessimized for other ZSTs, because some ZSTs cannot be safely constructed by
/// design, which potentially makes the following code unsound:
///
/// ```no_run
/// use crossmist::Object;
///
/// #[derive(Object)]
/// struct ZST;
///
/// // "Safely" constructs a ZST
/// fn conjure_zst() -> ZST {
///     helper.spawn().unwrap().join().unwrap()
/// }
///
/// #[crossmist::func]
/// fn helper() -> ZST {
///     std::process::exit(0)
/// }
///
/// #[crossmist::main]
/// fn main() {
///     conjure_zst();
/// }
/// ```
pub fn if_void<T>() -> Option<T> {
    T::if_void()
}

/// Initialize the crossmist runtime.
///
/// This function should always be called at the beginning of the program. It is automatically
/// called by `#[crossmist::main]`.
///
/// When crossmist spawns child processes, they start executing `main`. Calling [`init`] lets
/// crossmist passes control to the function that the process is actually supposed to be executing.
pub fn init() {
    if INITIALIZED.swap(true, Ordering::AcqRel) {
        return;
    }

    let mut args = std::env::args();
    if let Some(s) = args.next() {
        if s == "_crossmist_" {
            entry::crossmist_main(args);
        }
    }

    entry::start_root();
}
