use crate::{
    Object,
    owning_ref::{OwningRef, WithOwningRef},
    serde::{Deserializer, Serializer},
};
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, LinkedList, VecDeque};
use std::hash::{BuildHasher, Hash};
#[cfg(unix)]
use std::os::unix::io::OwnedFd;
#[cfg(windows)]
use std::os::windows::io::{OwnedHandle, OwnedSocket};
use std::time::{Duration, SystemTime};

macro_rules! impl_pod {
    ($([$($generics:tt)*])? for $t:ty) => {
        unsafe impl$(<$($generics)*>)? Object for $t {
            fn serialize_self(self, s: &mut Serializer) {
                s.write(unsafe {
                    std::slice::from_raw_parts((&raw const self).cast(), size_of::<Self>())
                });
            }
            fn serialize_slice(elements: OwningRef<'_, [Self]>, s: &mut Serializer) {
                s.write(unsafe {
                    std::slice::from_raw_parts(elements.as_ptr().cast(), size_of_val(&*elements))
                });
            }
            #[allow(unreachable_code)]
            unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
                unsafe {
                    d.read(size_of::<Self>()).as_ptr().cast::<Self>().read_unaligned()
                }
            }
        }
    };
}

impl_pod!(for ());
impl_pod!(for bool);
impl_pod!(for char);
impl_pod!([T] for std::marker::PhantomData<T>);
#[cfg(feature = "nightly")]
impl_pod!(for !);
impl_pod!(for std::convert::Infallible);
impl_pod!(for i8);
impl_pod!(for i16);
impl_pod!(for i32);
impl_pod!(for i64);
impl_pod!(for i128);
impl_pod!(for isize);
impl_pod!(for u8);
impl_pod!(for u16);
impl_pod!(for u32);
impl_pod!(for u64);
impl_pod!(for u128);
impl_pod!(for usize);
impl_pod!(for f32);
impl_pod!(for f64);
impl_pod!(for std::num::NonZeroI8);
impl_pod!(for std::num::NonZeroI16);
impl_pod!(for std::num::NonZeroI32);
impl_pod!(for std::num::NonZeroI64);
impl_pod!(for std::num::NonZeroI128);
impl_pod!(for std::num::NonZeroIsize);
impl_pod!(for std::num::NonZeroU8);
impl_pod!(for std::num::NonZeroU16);
impl_pod!(for std::num::NonZeroU32);
impl_pod!(for std::num::NonZeroU64);
impl_pod!(for std::num::NonZeroU128);
impl_pod!(for std::num::NonZeroUsize);

unsafe impl Object for Duration {
    fn serialize_self(self, s: &mut Serializer) {
        s.serialize(self.as_nanos());
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        Duration::from_nanos_u128(unsafe { d.deserialize() })
    }
}
// `Instant` cannot implement `Object` because it may be relative to the process start time.
unsafe impl Object for SystemTime {
    fn serialize_self(self, s: &mut Serializer) {
        // `SystemTime::MIN` is nightly-only, so for now we store the sign bit separately.
        let (is_negative, duration) = match self.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(duration) => (false, duration),
            Err(err) => (true, err.duration()),
        };
        s.serialize((is_negative, duration));
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        let (is_negative, duration) = unsafe { d.deserialize::<(bool, Duration)>() };
        if is_negative {
            SystemTime::UNIX_EPOCH - duration
        } else {
            SystemTime::UNIX_EPOCH + duration
        }
    }
}

unsafe impl Object for String {
    fn serialize_self(self, s: &mut Serializer) {
        s.serialize(self.into_bytes());
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        unsafe { String::from_utf8_unchecked(d.deserialize::<Vec<u8>>()) }
    }
}

unsafe impl Object for std::ffi::CString {
    fn serialize_self(self, s: &mut Serializer) {
        s.serialize(self.into_bytes());
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        unsafe { Self::from_vec_unchecked(d.deserialize::<Vec<u8>>()) }
    }
}

unsafe impl Object for std::ffi::OsString {
    fn serialize_self(self, s: &mut Serializer) {
        s.serialize(self.into_encoded_bytes());
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        unsafe { Self::from_encoded_bytes_unchecked(d.deserialize()) }
    }
}

#[cfg(all(doc, feature = "nightly"))]
#[doc(cfg(true), fake_variadic)]
/// This trait is implemented for tuples up to 20 items long.
unsafe impl<T: Object> Object for (T,) {
    fn serialize_self(self, _s: &mut Serializer) {}
    unsafe fn deserialize_self(_d: &mut Deserializer) -> Self {
        unimplemented!()
    }
}

macro_rules! impl_tuple {
    ($head:tt) => {};

    ($head:tt $($tail:tt)+) => {
        impl_tuple!($($tail)*);
        #[allow(nonstandard_style)]
        unsafe impl<$($tail: Object),*> Object for ($($tail,)*) {
            fn serialize_self(self, s: &mut Serializer) {
                let ($($tail,)*) = self;
                $( s.serialize($tail); )*
            }
            unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
                $( let $tail = unsafe { d.deserialize() }; )*
                ($($tail,)*)
            }
        }
    }
}

#[cfg(not(all(doc, feature = "nightly")))]
impl_tuple!(x T19 T18 T17 T16 T15 T14 T13 T12 T11 T10 T9 T8 T7 T6 T5 T4 T3 T2 T1 T0);

unsafe impl<T: Object> Object for Option<T> {
    fn serialize_self(self, s: &mut Serializer) {
        match self {
            None => s.serialize(false),
            Some(x) => {
                s.serialize(true);
                s.serialize(x);
            }
        }
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        unsafe { d.deserialize::<bool>().then(|| d.deserialize()) }
    }
}

unsafe impl Object for std::path::PathBuf {
    fn serialize_self(self, s: &mut Serializer) {
        s.serialize(self.into_os_string());
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        unsafe { d.deserialize::<std::ffi::OsString>() }.into()
    }
}

unsafe impl<T: Object, const N: usize> Object for [T; N] {
    fn serialize_self(self, s: &mut Serializer) {
        self.with_owning_ref(|slice| s.serialize_slice(slice));
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        core::array::from_fn(|_| unsafe { d.deserialize() })
    }
}

macro_rules! impl_sequence {
    ($ty:ident<$($params:ident),*> $(where $($bounds:tt)*)?) => {
        unsafe impl<$($params),*> Object for $ty<$($params),*>
        where
            T: Object,
            $($($bounds)*)?
        {
            fn serialize_self(self, s: &mut Serializer) {
                s.serialize(self.len());
                for item in self {
                    s.serialize(item);
                }
            }
            unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
                let len: usize = unsafe { d.deserialize() };
                (0..len).map(|_| unsafe { d.deserialize() }).collect()
            }
        }
    }
}

macro_rules! impl_map {
    ($ty:ident<$($params:ident),*> $(where $($bounds:tt)*)?) => {
        unsafe impl<$($params),*> Object for $ty<$($params),*>
        where
            K: Object,
            V: Object,
            $($($bounds)*)?
        {
            fn serialize_self(self, s: &mut Serializer) {
                s.serialize(self.len());
                for (key, value) in self {
                    s.serialize(key);
                    s.serialize(value);
                }
            }
            unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
                let len: usize = unsafe { d.deserialize() };
                (0..len).map(|_| unsafe { d.deserialize() }).collect()
            }
        }
    }
}

impl_sequence!(Vec<T>);
impl_sequence!(BinaryHeap<T> where T: Ord);
impl_sequence!(BTreeSet<T> where T: Eq + Ord);
impl_sequence!(LinkedList<T>);
impl_sequence!(HashSet<T, S> where T: Eq + Hash, S: BuildHasher + Default);
impl_sequence!(VecDeque<T>);
impl_map!(BTreeMap<K, V> where K: Ord);
impl_map!(HashMap<K, V, S> where K: Eq + Hash, S: BuildHasher + Default);

unsafe impl<T: Object, E: Object> Object for Result<T, E> {
    fn serialize_self(self, s: &mut Serializer) {
        match self {
            Ok(ok) => {
                s.serialize(true);
                s.serialize(ok);
            }
            Err(err) => {
                s.serialize(false);
                s.serialize(err);
            }
        }
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        unsafe {
            if d.deserialize::<bool>() {
                Ok(d.deserialize())
            } else {
                Err(d.deserialize())
            }
        }
    }
}

#[cfg(unix)]
unsafe impl Object for OwnedFd {
    fn serialize_self(self, s: &mut Serializer) {
        s.fds.push(self);
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        d.fds
            .next()
            .expect("Mismatched (de)serialization of OwnedFd")
    }
}

#[cfg(windows)]
unsafe impl Object for OwnedHandle {
    fn serialize_self(self, s: &mut Serializer) {
        s.handles.push(self);
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        d.handles
            .next()
            .expect("Mismatched (de)serialization of OwnedHandle")
    }
}

#[cfg(windows)]
unsafe impl Object for OwnedSocket {
    fn serialize_self(self, s: &mut Serializer) {
        s.sockets.push(self);
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        d.sockets
            .next()
            .expect("Mismatched (de)serialization of OwnedSocket")
    }
}

unsafe impl Object for std::fs::File {
    fn serialize_self(self, s: &mut Serializer) {
        #[cfg(unix)]
        s.serialize(OwnedFd::from(self));
        #[cfg(windows)]
        s.serialize(OwnedHandle::from(self));
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        #[cfg(unix)]
        let raw = unsafe { d.deserialize::<OwnedFd>() };
        #[cfg(windows)]
        let raw = unsafe { d.deserialize::<OwnedHandle>() };
        raw.into()
    }
}

#[cfg(feature = "tokio")]
unsafe impl Object for tokio::fs::File {
    fn serialize_self(self, s: &mut Serializer) {
        s.serialize(self.try_into_std().expect("cannot serialize File"));
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        unsafe { d.deserialize::<std::fs::File>() }.into()
    }
}

// https://github.com/smol-rs/async-fs/issues/49
// #[cfg(feature = "smol")]
// unsafe impl Object for async_fs::File {
//     fn serialize_self(self, s: &mut Serializer) {
//         s.serialize(std::fs::File::from(self));
//     }
//     unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
//         unsafe { d.deserialize::<std::fs::File>() }.into()
//     }
// }

#[cfg(unix)]
unsafe impl Object for std::os::unix::net::UnixStream {
    fn serialize_self(self, s: &mut Serializer) {
        s.serialize(OwnedFd::from(self));
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        unsafe { d.deserialize::<OwnedFd>() }.into()
    }
}

#[cfg(all(unix, feature = "tokio"))]
unsafe impl Object for tokio::net::UnixStream {
    fn serialize_self(self, s: &mut Serializer) {
        s.serialize(self.into_std().expect("cannot serialize UnixStream"));
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        Self::from_std(unsafe { d.deserialize() }).expect("cannot deserialize UnixStream")
    }
}

unsafe impl Object for std::net::TcpStream {
    fn serialize_self(self, s: &mut Serializer) {
        #[cfg(unix)]
        s.serialize(OwnedFd::from(self));
        #[cfg(windows)]
        s.serialize(OwnedSocket::from(self));
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        #[cfg(unix)]
        let raw = unsafe { d.deserialize::<OwnedFd>() };
        #[cfg(windows)]
        let raw = unsafe { d.deserialize::<OwnedSocket>() };
        raw.into()
    }
}

#[cfg(feature = "tokio")]
unsafe impl Object for tokio::net::TcpStream {
    fn serialize_self(self, s: &mut Serializer) {
        s.serialize(self.into_std().expect("cannot serialize TcpStream"));
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        Self::from_std(unsafe { d.deserialize() }).expect("cannot deserialize TcpStream")
    }
}

#[cfg(all(unix, feature = "smol"))]
unsafe impl<T: std::os::fd::AsFd + Object> Object for async_io::Async<T> {
    fn serialize_self(self, s: &mut Serializer) {
        s.serialize(self.into_inner().expect("cannot serialize Async"))
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        async_io::Async::new(unsafe { d.deserialize() }).expect("cannot deserialize Async")
    }
}

#[cfg(all(windows, feature = "smol"))]
unsafe impl<T: std::os::windows::io::AsSocket + Object> Object for async_io::Async<T> {
    fn serialize_self(self, s: &mut Serializer) {
        s.serialize(self.into_inner().expect("cannot serialize Async"))
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        async_io::Async::new(unsafe { d.deserialize() }).expect("cannot deserialize Async")
    }
}
