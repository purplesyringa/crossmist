pub use ctor::ctor;

use crate::entry;
use lazy_static::lazy_static;
use std::sync::RwLock;

lazy_static! {
    pub static ref MAIN_ENTRY: RwLock<Option<fn() -> i32>> = RwLock::new(None);
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

pub fn main() {
    let mut args = std::env::args();
    if let Some(s) = args.next() {
        if s == "_multiprocessing_" {
            entry::multiprocessing_main(args);
        }
    }

    std::process::exit(MAIN_ENTRY
        .read()
        .expect("Failed to acquire read access to MAIN_ENTRY")
        .expect(
            "MAIN_ENTRY was not registered: is #[multiprocessing::main] missing?",
        )());
}
