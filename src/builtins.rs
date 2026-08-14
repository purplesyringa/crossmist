#[cfg(windows)]
use crate::handles::RawHandle;
use crate::{
    Deserializer, Object, Serializer,
    handles::{AsHandle, OwnedHandle},
};
use paste::paste;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, LinkedList, VecDeque};
use std::hash::{BuildHasher, Hash};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

macro_rules! impl_pod {
    ($([$($generics:tt)*])? for $t:ty) => {
        unsafe impl$(<$($generics)*>)? Object for $t {
            fn serialize_self<'a>(&'a self, s: &mut Serializer<'a>) {
                s.write(unsafe {
                    std::slice::from_raw_parts(core::ptr::from_ref(self).cast(), size_of::<Self>())
                });
            }
            fn serialize_slice<'a>(elements: &'a [Self], s: &mut Serializer<'a>) {
                s.write(unsafe {
                    std::slice::from_raw_parts(elements.as_ptr().cast(), size_of_val(elements))
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
    fn serialize_self<'a>(&'a self, s: &mut Serializer<'a>) {
        s.serialize_temporary(self.as_nanos());
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        Duration::from_nanos_u128(unsafe { d.deserialize() })
    }
}
// `Instant` cannot implement `Object` because it may be relative to the process start time.
unsafe impl Object for SystemTime {
    fn serialize_self<'a>(&'a self, s: &mut Serializer<'a>) {
        // `SystemTime::MIN` is nightly-only, so for now we store the sign bit separately.
        let (is_negative, duration) = match self.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(duration) => (false, duration),
            Err(err) => (true, err.duration()),
        };
        s.serialize_temporary((is_negative, duration));
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
    fn serialize_self<'a>(&'a self, s: &mut Serializer<'a>) {
        s.serialize_temporary(self.len());
        s.serialize_slice(self.as_bytes());
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        unsafe { String::from_utf8_unchecked(d.deserialize::<Vec<u8>>()) }
    }
}

unsafe impl Object for std::ffi::CString {
    fn serialize_self<'a>(&'a self, s: &mut Serializer<'a>) {
        let bytes = self.as_bytes();
        s.serialize_temporary(bytes.len());
        s.serialize_slice(bytes);
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        unsafe { Self::from_vec_unchecked(d.deserialize::<Vec<u8>>()) }
    }
}

unsafe impl Object for std::ffi::OsString {
    fn serialize_self<'a>(&'a self, s: &mut Serializer<'a>) {
        let bytes = self.as_encoded_bytes();
        s.serialize_temporary(bytes.len());
        s.serialize_slice(bytes);
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        unsafe { Self::from_encoded_bytes_unchecked(d.deserialize()) }
    }
}

macro_rules! serialize_rev {
    ($s:tt,) => {};
    ($s:tt, $head:expr, $($tail:tt)*) => {
        serialize_rev!($s, $($tail)*);
        $s.serialize(&$head);
    };
}

#[cfg(all(doc, feature = "nightly"))]
#[doc(cfg(true), fake_variadic)]
/// This trait is implemented for tuples up to 20 items long.
unsafe impl<T: Object> Object for (T,) {
    fn serialize_self<'a>(&'a self, _s: &mut Serializer<'a>) {}
    unsafe fn deserialize_self(_d: &mut Deserializer) -> Self {
        unimplemented!()
    }
}

macro_rules! impl_tuple {
    () => {};

    ($head:tt $($tail:tt)*) => {
        impl_tuple!($($tail)*);

        paste! {
            unsafe impl<$([<T $tail>]: Object),*> Object for ($([<T $tail>],)*) {
                #[allow(unused_variables)]
                fn serialize_self<'a>(&'a self, s: &mut Serializer<'a>) {
                    serialize_rev!(s, $(self.$tail,)*);
                }
                #[allow(unused_variables, clippy::unused_unit)]
                unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
                    $( let [<x $tail>] = unsafe { d.deserialize() }; )*
                    ($([<x $tail>],)*)
                }
            }
        }
    }
}

#[cfg(not(all(doc, feature = "nightly")))]
impl_tuple!(x 19 18 17 16 15 14 13 12 11 10 9 8 7 6 5 4 3 2 1 0);

unsafe impl<T: Object> Object for Option<T> {
    fn serialize_self<'a>(&'a self, s: &mut Serializer<'a>) {
        match self {
            None => s.serialize_temporary(false),
            Some(x) => {
                s.serialize_temporary(true);
                s.serialize(x);
            }
        }
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        unsafe { d.deserialize::<bool>().then(|| d.deserialize()) }
    }
}

unsafe impl<T: 'static + Object> Object for Rc<T> {
    fn serialize_self<'a>(&'a self, s: &mut Serializer<'a>) {
        match s.learn_cyclic(Rc::as_ptr(self).cast()) {
            None => {
                s.serialize_temporary(0usize);
                s.serialize(&**self);
            }
            Some(id) => {
                s.serialize_temporary(id.get());
            }
        }
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        unsafe {
            let id = d.deserialize::<usize>();
            match std::num::NonZeroUsize::new(id) {
                None => {
                    let rc = Self::new(d.deserialize());
                    d.learn_cyclic(rc.clone());
                    rc
                }
                Some(id) => d.get_cyclic::<Rc<T>>(id).clone(),
            }
        }
    }
}

unsafe impl<T: 'static + Object> Object for Arc<T> {
    fn serialize_self<'a>(&'a self, s: &mut Serializer<'a>) {
        match s.learn_cyclic(Arc::as_ptr(self).cast()) {
            None => {
                s.serialize_temporary(0usize);
                s.serialize(&**self);
            }
            Some(id) => {
                s.serialize_temporary(id.get());
            }
        }
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        unsafe {
            let id = d.deserialize::<usize>();
            match std::num::NonZeroUsize::new(id) {
                None => {
                    let rc = Self::new(d.deserialize());
                    d.learn_cyclic(rc.clone());
                    rc
                }
                Some(id) => d.get_cyclic::<Arc<T>>(id).clone(),
            }
        }
    }
}

unsafe impl Object for std::path::PathBuf {
    fn serialize_self<'a>(&'a self, s: &mut Serializer<'a>) {
        let bytes = self.as_os_str().as_encoded_bytes();
        s.serialize_temporary(bytes.len());
        s.serialize_slice(bytes);
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        unsafe { d.deserialize::<std::ffi::OsString>() }.into()
    }
}

unsafe impl<T: Object, const N: usize> Object for [T; N] {
    fn serialize_self<'a>(&'a self, s: &mut Serializer<'a>) {
        s.serialize_slice(self);
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
            fn serialize_self<'a>(&'a self, s: &mut Serializer<'a>) {
                s.serialize_temporary(self.len());
                for item in self.iter() {
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
            fn serialize_self<'a>(&'a self, s: &mut Serializer<'a>) {
                s.serialize_temporary(self.len());
                for (key, value) in self.iter() {
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
    fn serialize_self<'a>(&'a self, s: &mut Serializer<'a>) {
        match self {
            Ok(ok) => {
                s.serialize_temporary(true);
                s.serialize(ok);
            }
            Err(err) => {
                s.serialize_temporary(false);
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

unsafe impl Object for OwnedHandle {
    fn serialize_self<'a>(&'a self, s: &mut Serializer<'a>) {
        s.serialize_handle(self.as_handle());
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        d.handles
            .next()
            .expect("Mismatched calls to serialize_handle/deserialize_handle")
    }
}

unsafe impl Object for std::fs::File {
    fn serialize_self<'a>(&'a self, s: &mut Serializer<'a>) {
        s.serialize_handle(self.as_handle());
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        unsafe { d.deserialize::<OwnedHandle>() }.into()
    }
}

#[cfg(feature = "tokio")]
unsafe impl Object for tokio::fs::File {
    fn serialize_self<'a>(&'a self, s: &mut Serializer<'a>) {
        s.serialize_handle(self.as_handle());
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        unsafe { d.deserialize::<std::fs::File>() }.into()
    }
}

#[cfg(feature = "smol")]
unsafe impl Object for async_fs::File {
    fn serialize_self<'a>(&'a self, s: &mut Serializer<'a>) {
        s.serialize_handle(self.as_handle());
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        unsafe { d.deserialize::<std::fs::File>() }.into()
    }
}

#[cfg(unix)]
unsafe impl Object for std::os::unix::net::UnixStream {
    fn serialize_self<'a>(&'a self, s: &mut Serializer<'a>) {
        s.serialize_handle(self.as_handle());
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        unsafe { d.deserialize::<OwnedHandle>() }.into()
    }
}

#[cfg(all(unix, feature = "tokio"))]
unsafe impl Object for tokio::net::UnixStream {
    fn serialize_self<'a>(&'a self, s: &mut Serializer<'a>) {
        s.serialize_handle(self.as_handle());
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        Self::from_std(unsafe { d.deserialize() }).expect("cannot deserialize UnixStream")
    }
}

#[cfg(all(unix, feature = "smol"))]
unsafe impl<T: std::os::fd::AsFd + Object> Object for async_io::Async<T> {
    fn serialize_self<'a>(&'a self, s: &mut Serializer<'a>) {
        s.serialize(self.get_ref())
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        async_io::Async::new(unsafe { d.deserialize() }).expect("cannot deserialize Async")
    }
}

#[cfg(windows)]
impl_pod!(for RawHandle);
