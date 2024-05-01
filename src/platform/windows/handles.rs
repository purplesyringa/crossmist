//! Cross-platform alternative to file descriptors.
//!
//! You most likely won't ever need to use this: `#[derive(Object)]` shall take care of serializing
//! and deserializing [`std::fs::File`] and similar objects. If, however, you store raw file
//! descriptors and implement serialization yourself, using this module might be a sane choice.

use std::os::windows::io;
use windows::Win32::Foundation::HANDLE;

// We use HANDLE from 'windows' crate instead of io::RawHandle because the latter is just an alias
// to a pointer and not a newtype, so if we implement traits for it, chaos is likely to ensue. Also,
// *mut c_void is not Sync + Send, so lazy_static! and coroutines (likely) fail.

pub trait FromRawHandle: io::FromRawHandle {
    unsafe fn from_raw_handle(handle: RawHandle) -> Self
    where
        Self: Sized,
    {
        <Self as io::FromRawHandle>::from_raw_handle(handle.0 as io::RawHandle)
    }
}
pub trait IntoRawHandle: io::IntoRawHandle {
    fn into_raw_handle(self) -> RawHandle
    where
        Self: Sized,
    {
        HANDLE(<Self as io::IntoRawHandle>::into_raw_handle(self) as isize)
    }
}
pub trait AsRawHandle: io::AsRawHandle {
    fn as_raw_handle(&self) -> RawHandle {
        HANDLE(<Self as io::AsRawHandle>::as_raw_handle(self) as isize)
    }
}

impl<T: io::FromRawHandle> FromRawHandle for T {}
impl<T: io::IntoRawHandle> IntoRawHandle for T {}
impl<T: io::AsRawHandle> AsRawHandle for T {}

pub type RawHandle = HANDLE;
pub type OwnedHandle = io::OwnedHandle;
