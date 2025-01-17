#[cfg(windows)]
use crate::handles::RawHandle;
#[cfg(feature = "tokio")]
use crate::handles::{FromRawHandle, IntoRawHandle};
use crate::{
    handles::{AsHandle, OwnedHandle},
    pod::PlainOldData,
    Deserializer, NonTrivialObject, Object, Serializer,
};
use paste::paste;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, LinkedList, VecDeque};
use std::hash::{BuildHasher, Hash};
use std::io::Result;
use std::mem::MaybeUninit;
use std::os::raw::c_void;
use std::rc::Rc;
use std::sync::Arc;

macro_rules! impl_pod {
    ([$($generics:tt)*] for $t:ty) => {
        unsafe impl<$($generics)*> NonTrivialObject for $t {
            fn serialize_self_non_trivial<'a>(&'a self, _s: &mut Serializer<'a>) {
                unreachable!()
            }
            unsafe fn deserialize_self_non_trivial(_d: &mut Deserializer) -> Result<Self> {
                unreachable!()
            }
        }
        unsafe impl<$($generics)*> PlainOldData for $t {}
    };
    (for $t:ty) => {
        unsafe impl NonTrivialObject for $t {
            fn serialize_self_non_trivial<'a>(&'a self, _s: &mut Serializer<'a>) {
                unreachable!()
            }
            unsafe fn deserialize_self_non_trivial(_d: &mut Deserializer) -> Result<Self> {
                unreachable!()
            }
        }
        unsafe impl PlainOldData for $t {}
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
impl_pod!(for std::time::Duration);
impl_pod!(for std::time::Instant);
impl_pod!(for std::time::SystemTime);

unsafe impl NonTrivialObject for String {
    fn serialize_self_non_trivial<'a>(&'a self, s: &mut Serializer<'a>) {
        s.serialize_temporary(self.len());
        s.serialize_slice(self.as_bytes());
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Result<Self> {
        Ok(unsafe { String::from_utf8_unchecked(d.deserialize::<Vec<u8>>()?) })
    }
}

unsafe impl NonTrivialObject for std::ffi::CString {
    fn serialize_self_non_trivial<'a>(&'a self, s: &mut Serializer<'a>) {
        let bytes = self.as_bytes();
        s.serialize_temporary(bytes.len());
        s.serialize_slice(bytes);
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Result<Self> {
        Ok(unsafe { Self::from_vec_unchecked(d.deserialize::<Vec<u8>>()?) })
    }
}

unsafe impl NonTrivialObject for std::ffi::OsString {
    fn serialize_self_non_trivial<'a>(&'a self, s: &mut Serializer<'a>) {
        let bytes = self.as_encoded_bytes();
        s.serialize_temporary(bytes.len());
        s.serialize_slice(bytes);
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Result<Self> {
        Ok(unsafe { Self::from_encoded_bytes_unchecked(d.deserialize()?) })
    }
}

macro_rules! serialize_rev {
    ($s:tt, $self:tt,) => {};

    ($s:tt, $self:tt, $head:tt $($tail:tt)*) => {
        serialize_rev!($s, $self, $($tail)*);
        $s.serialize(&$self.$head);
    }
}

#[cfg(docsrs)]
#[doc(cfg(true), fake_variadic)]
/// This trait is implemented for tuples up to 20 items long.
unsafe impl<T: Object> NonTrivialObject for (T,) {
    fn serialize_self_non_trivial<'a>(&'a self, _s: &mut Serializer<'a>) {}
    unsafe fn deserialize_self_non_trivial(_d: &mut Deserializer) -> Result<Self> {
        unimplemented!()
    }
}

#[cfg(docsrs)]
#[doc(cfg(true), fake_variadic)]
/// This trait is implemented for tuples up to 20 items long.
unsafe impl<T: PlainOldData> PlainOldData for (T,) {}

macro_rules! impl_serialize_for_tuple {
    () => {};

    ($head:tt $($tail:tt)*) => {
        impl_serialize_for_tuple!($($tail)*);

        #[cfg(not(docsrs))]
        paste! {
            unsafe impl<$([<T $tail>]: Object),*> NonTrivialObject for ($([<T $tail>],)*) {
                #[allow(unused_variables)]
                fn serialize_self_non_trivial<'a>(&'a self, s: &mut Serializer<'a>) {
                    serialize_rev!(s, self, $($tail)*);
                }
                #[allow(unused_variables)]
                #[allow(clippy::unused_unit)]
                unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Result<Self> {
                    $( let [<x $tail>] = d.deserialize()?; )*
                    Ok(($([<x $tail>],)*))
                }
            }
            unsafe impl<$([<T $tail>]: PlainOldData),*> PlainOldData for ($([<T $tail>],)*) {}
        }
    }
}

impl_serialize_for_tuple!(x 19 18 17 16 15 14 13 12 11 10 9 8 7 6 5 4 3 2 1 0);

unsafe impl<T: Object> NonTrivialObject for Option<T> {
    fn serialize_self_non_trivial<'a>(&'a self, s: &mut Serializer<'a>) {
        match self {
            None => s.serialize_temporary(false),
            Some(ref x) => {
                s.serialize_temporary(true);
                s.serialize(x);
            }
        }
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Result<Self> {
        if d.deserialize::<bool>()? {
            d.deserialize().map(Some)
        } else {
            Ok(None)
        }
    }
}
unsafe impl<T: PlainOldData> PlainOldData for Option<T> {}

unsafe impl<T: 'static + Object> NonTrivialObject for Rc<T> {
    fn serialize_self_non_trivial<'a>(&'a self, s: &mut Serializer<'a>) {
        match s.learn_cyclic(Rc::as_ptr(self) as *const c_void) {
            None => {
                s.serialize_temporary(0usize);
                s.serialize(&**self);
            }
            Some(id) => {
                s.serialize_temporary(id);
            }
        }
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Result<Self> {
        let id = d.deserialize::<usize>()?;
        match std::num::NonZeroUsize::new(id) {
            None => {
                let rc = Self::new(d.deserialize()?);
                d.learn_cyclic(rc.clone());
                Ok(rc)
            }
            Some(id) => Ok(d.get_cyclic::<Rc<T>>(id).clone()),
        }
    }
}

unsafe impl<T: 'static + Object> NonTrivialObject for Arc<T> {
    fn serialize_self_non_trivial<'a>(&'a self, s: &mut Serializer<'a>) {
        match s.learn_cyclic(Arc::as_ptr(self) as *const c_void) {
            None => {
                s.serialize_temporary(0usize);
                s.serialize(&**self);
            }
            Some(id) => {
                s.serialize_temporary(id);
            }
        }
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Result<Self> {
        let id = d.deserialize::<usize>()?;
        match std::num::NonZeroUsize::new(id) {
            None => {
                let rc = Self::new(d.deserialize()?);
                d.learn_cyclic(rc.clone());
                Ok(rc)
            }
            Some(id) => Ok(d.get_cyclic::<Arc<T>>(id).clone()),
        }
    }
}

unsafe impl NonTrivialObject for std::path::PathBuf {
    fn serialize_self_non_trivial<'a>(&'a self, s: &mut Serializer<'a>) {
        let bytes = self.as_os_str().as_encoded_bytes();
        s.serialize_temporary(bytes.len());
        s.serialize_slice(bytes);
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Result<Self> {
        Ok(d.deserialize::<std::ffi::OsString>()?.into())
    }
}

unsafe impl<T: Object, const N: usize> NonTrivialObject for [T; N] {
    fn serialize_self_non_trivial<'a>(&'a self, s: &mut Serializer<'a>) {
        s.serialize_slice(self);
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Result<Self> {
        // array::try_map is not stabilized yet
        let mut array: MaybeUninit<[T; N]> = MaybeUninit::uninit();
        let array_ptr: *mut T = array.as_mut_ptr() as *mut T;
        for i in 0..N {
            match d.deserialize() {
                Ok(value) => array_ptr.add(i).write(value),
                Err(e) => {
                    for j in 0..i {
                        array_ptr.add(j).drop_in_place();
                    }
                    return Err(e);
                }
            }
        }
        Ok(array.assume_init())
    }
}
unsafe impl<T: PlainOldData, const N: usize> PlainOldData for [T; N] {}

unsafe impl<T: Object> NonTrivialObject for Vec<T> {
    fn serialize_self_non_trivial<'a>(&'a self, s: &mut Serializer<'a>) {
        s.serialize_temporary(self.len());
        s.serialize_slice(self.as_slice())
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Result<Self> {
        let size: usize = d.deserialize()?;
        let mut seq = Vec::with_capacity(size);
        for _ in 0..size {
            seq.push(d.deserialize()?);
        }
        Ok(seq)
    }
}

macro_rules! impl_serialize_for_sequence {
    (
        $ty:ident < T $(: $tbound1:ident $(+ $tbound2:ident)*)* $(, $typaram:ident : $bound1:ident $(+ $bound2:ident)*)* >,
        $seq:ident,
        $size:ident,
        $with_capacity:expr,
        $push:expr
    ) => {
        unsafe impl<T: Object $(+ $tbound1 $(+ $tbound2)*)* $(, $typaram: $bound1 $(+ $bound2)*,)*> NonTrivialObject
            for $ty<T $(, $typaram)*>
        {
            fn serialize_self_non_trivial<'a>(&'a self, s: &mut Serializer<'a>) {
                s.serialize_temporary(self.len());
                for item in self.iter() {
                    s.serialize(item);
                }
            }
            unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Result<Self> {
                let $size: usize = d.deserialize()?;
                let mut $seq = $with_capacity;
                for _ in 0..$size {
                    $push(&mut $seq, d.deserialize()?);
                }
                Ok($seq)
            }
        }
    }
}

macro_rules! impl_serialize_for_map {
    (
        $ty:ident <
            K $(: $kbound1:ident $(+ $kbound2:ident)*)*,
            V
            $(, $typaram:ident : $bound1:ident $(+ $bound2:ident)*)*
        >,
        $size:ident,
        $with_capacity:expr
    ) => {
        unsafe impl<
            K: Object $(+ $kbound1 $(+ $kbound2)*)*,
            V: Object
            $(, $typaram: $bound1 $(+ $bound2)*,)*
        > NonTrivialObject
            for $ty<K, V $(, $typaram)*>
        {
            fn serialize_self_non_trivial<'a>(&'a self, s: &mut Serializer<'a>) {
                s.serialize_temporary(self.len());
                for (key, value) in self.iter() {
                    s.serialize(key);
                    s.serialize(value);
                }
            }
            unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Result<Self> {
                let $size: usize = d.deserialize()?;
                let mut map = $with_capacity;
                for _ in 0..$size {
                    map.insert(d.deserialize()?, d.deserialize()?);
                }
                Ok(map)
            }
        }
    }
}

impl_serialize_for_sequence!(
    BinaryHeap<T: Ord>,
    seq,
    size,
    BinaryHeap::with_capacity(size),
    BinaryHeap::push
);
impl_serialize_for_sequence!(
    BTreeSet<T: Eq + Ord>,
    seq,
    size,
    BTreeSet::new(),
    BTreeSet::insert
);
impl_serialize_for_sequence!(
    LinkedList<T>,
    seq,
    size,
    LinkedList::new(),
    LinkedList::push_back
);
impl_serialize_for_sequence!(
    HashSet<T: Eq + Hash, S: BuildHasher + Default>,
    seq,
    size,
    HashSet::with_capacity_and_hasher(size, S::default()),
    HashSet::insert
);
impl_serialize_for_sequence!(
    VecDeque<T>,
    seq,
    size,
    VecDeque::with_capacity(size),
    VecDeque::push_back
);
impl_serialize_for_map!(BTreeMap<K: Ord, V>, size, BTreeMap::new());
impl_serialize_for_map!(
    HashMap<K: Eq + Hash, V, S: BuildHasher + Default>,
    size,
    HashMap::with_capacity_and_hasher(size, S::default())
);

unsafe impl<T: Object, E: Object> NonTrivialObject for std::result::Result<T, E> {
    fn serialize_self_non_trivial<'a>(&'a self, s: &mut Serializer<'a>) {
        match self {
            Ok(ref ok) => {
                s.serialize_temporary(true);
                s.serialize(ok);
            }
            Err(ref err) => {
                s.serialize_temporary(false);
                s.serialize(err);
            }
        }
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Result<Self> {
        Ok(if d.deserialize::<bool>()? {
            Ok(d.deserialize()?)
        } else {
            Err(d.deserialize()?)
        })
    }
}
unsafe impl<T: PlainOldData, E: PlainOldData> PlainOldData for std::result::Result<T, E> {}

unsafe impl NonTrivialObject for OwnedHandle {
    fn serialize_self_non_trivial<'a>(&'a self, s: &mut Serializer<'a>) {
        s.serialize_handle(self.as_handle());
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Result<Self> {
        Ok(d.handles
            .next()
            .expect("Mismatched calls to serialize_handle/deserialize_handle"))
    }
}

unsafe impl NonTrivialObject for std::fs::File {
    fn serialize_self_non_trivial<'a>(&'a self, s: &mut Serializer<'a>) {
        s.serialize_handle(self.as_handle());
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Result<Self> {
        Ok(d.deserialize::<OwnedHandle>()?.into())
    }
}

#[cfg(feature = "tokio")]
unsafe impl NonTrivialObject for tokio::fs::File {
    fn serialize_self_non_trivial<'a>(&'a self, s: &mut Serializer<'a>) {
        s.serialize_handle(self.as_handle());
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Result<Self> {
        Ok(unsafe {
            <Self as FromRawHandle>::from_raw_handle(
                d.deserialize::<OwnedHandle>()?.into_raw_handle(),
            )
        })
    }
}

#[cfg(feature = "smol")]
unsafe impl NonTrivialObject for async_fs::File {
    fn serialize_self_non_trivial<'a>(&'a self, s: &mut Serializer<'a>) {
        s.serialize_handle(self.as_handle());
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Result<Self> {
        Ok(d.deserialize::<std::fs::File>()?.into())
    }
}

#[cfg(unix)]
unsafe impl NonTrivialObject for std::os::unix::net::UnixStream {
    fn serialize_self_non_trivial<'a>(&'a self, s: &mut Serializer<'a>) {
        s.serialize_handle(self.as_handle());
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Result<Self> {
        Ok(d.deserialize::<OwnedHandle>()?.into())
    }
}

#[cfg(all(unix, feature = "tokio"))]
unsafe impl NonTrivialObject for tokio::net::UnixStream {
    fn serialize_self_non_trivial<'a>(&'a self, s: &mut Serializer<'a>) {
        s.serialize_handle(self.as_handle());
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Result<Self> {
        Self::from_std(d.deserialize()?)
    }
}

#[cfg(all(unix, feature = "smol"))]
unsafe impl<T: 'static + std::os::fd::AsFd + Object> NonTrivialObject for async_io::Async<T> {
    fn serialize_self_non_trivial<'a>(&'a self, s: &mut Serializer<'a>) {
        s.serialize(self.get_ref())
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Result<Self> {
        async_io::Async::new(d.deserialize::<T>()?)
    }
}

#[cfg(windows)]
impl_pod!(for RawHandle);
