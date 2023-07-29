#[cfg(windows)]
use crate::handles::RawHandle;
#[cfg(feature = "tokio")]
use crate::handles::{FromRawHandle, IntoRawHandle};
use crate::{
    handles::{AsRawHandle, OwnedHandle},
    pod::{EfficientObject, PlainOldData},
    Deserializer, Object, Serializer,
};
use paste::paste;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, LinkedList, VecDeque};
use std::hash::{BuildHasher, Hash};
use std::os::raw::c_void;
use std::rc::Rc;
use std::sync::Arc;

macro_rules! impl_pod {
    ([$($generics:tt)*] for $t:ty) => {
        impl<$($generics)*> Object for $t {
            fn serialize_self(&self, _s: &mut Serializer) {
                unreachable!()
            }
            fn deserialize_self(_d: &mut Deserializer) -> Self {
                unreachable!()
            }
            fn deserialize_on_heap<'a>(&self, _d: &mut Deserializer) -> Box<dyn Object + 'a> {
                unreachable!()
            }
        }
        impl<$($generics)*> PlainOldData for $t {}
    };
    (for $t:ty) => {
        impl Object for $t {
            fn serialize_self(&self, _s: &mut Serializer) {
                unreachable!()
            }
            fn deserialize_self(_d: &mut Deserializer) -> Self {
                unreachable!()
            }
            fn deserialize_on_heap<'a>(&self, _d: &mut Deserializer) -> Box<dyn Object + 'a> {
                unreachable!()
            }
        }
        impl PlainOldData for $t {}
    };
}

impl_pod!(for bool);
impl_pod!(for char);
impl_pod!([T] for std::marker::PhantomData<T>);
impl_pod!(for !);
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

impl Object for String {
    fn serialize_self(&self, s: &mut Serializer) {
        // XXX: unnecessary heap usage
        s.serialize(&Vec::from(self.as_bytes()))
    }
    fn deserialize_self(d: &mut Deserializer) -> Self {
        // XXX: unnecessary heap usage
        std::str::from_utf8(&d.deserialize::<Vec<u8>>())
            .expect("Failed to deserialize string")
            .to_string()
    }
    fn deserialize_on_heap<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a> {
        Box::new(Self::deserialize_self(d))
    }
}

impl Object for std::ffi::CString {
    fn serialize_self(&self, s: &mut Serializer) {
        // XXX: unnecessary heap usage
        s.serialize(&Vec::from(self.as_bytes()))
    }
    fn deserialize_self(d: &mut Deserializer) -> Self {
        // XXX: unnecessary heap usage
        Self::new(
            std::str::from_utf8(&d.deserialize::<Vec<u8>>())
                .expect("Failed to deserialize CString (UTF-8 decoding)"),
        )
        .expect("Failed to deserialize CString (null byte in the middle)")
    }
    fn deserialize_on_heap<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a> {
        Box::new(Self::deserialize_self(d))
    }
}

#[cfg(unix)]
impl Object for std::ffi::OsString {
    fn serialize_self(&self, s: &mut Serializer) {
        use std::os::unix::ffi::OsStringExt;
        // XXX: unnecessary heap usage
        s.serialize(&self.clone().into_vec())
    }
    fn deserialize_self(d: &mut Deserializer) -> Self {
        use std::os::unix::ffi::OsStringExt;
        Self::from_vec(d.deserialize())
    }
    fn deserialize_on_heap<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a> {
        Box::new(Self::deserialize_self(d))
    }
}
#[cfg(windows)]
impl Object for std::ffi::OsString {
    fn serialize_self(&self, s: &mut Serializer) {
        use std::os::windows::ffi::OsStrExt;
        // XXX: unnecessary heap usage
        s.serialize(&self.encode_wide().collect::<Vec<u16>>())
    }
    fn deserialize_self(d: &mut Deserializer) -> Self {
        use std::os::windows::ffi::OsStringExt;
        // XXX: unnecessary heap usage
        let vec: Vec<u16> = d.deserialize();
        Self::from_wide(&vec)
    }
    fn deserialize_on_heap<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a> {
        Box::new(Self::deserialize_self(d))
    }
}

macro_rules! serialize_rev {
    ($s:tt, $self:tt,) => {};

    ($s:tt, $self:tt, $head:tt $($tail:tt)*) => {
        serialize_rev!($s, $self, $($tail)*);
        $s.serialize(&$self.$head);
    }
}

macro_rules! impl_serialize_for_tuple {
    () => {};

    ($head:tt $($tail:tt)*) => {
        impl_serialize_for_tuple!($($tail)*);

        paste! {
            impl<$([<T $tail>]: Object),*> Object for ($([<T $tail>],)*) {
                #[allow(unused_variables)]
                fn serialize_self(&self, s: &mut Serializer) {
                    serialize_rev!(s, self, $($tail)*);
                }
                #[allow(unused_variables)]
                fn deserialize_self(d: &mut Deserializer) -> Self {
                    $( let [<x $tail>] = d.deserialize(); )*
                    ($([<x $tail>],)*)
                }
                fn deserialize_on_heap<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a>
                where
                    $([<T $tail>]: 'a),*
                {
                    Box::new(Self::deserialize_self(d))
                }
            }
            impl<$([<T $tail>]: PlainOldData),*> PlainOldData for ($([<T $tail>],)*) {}
        }
    }
}

impl_serialize_for_tuple!(x 19 18 17 16 15 14 13 12 11 10 9 8 7 6 5 4 3 2 1 0);

impl<T: Object> Object for Option<T> {
    fn serialize_self(&self, s: &mut Serializer) {
        match self {
            None => s.serialize(&false),
            Some(ref x) => {
                s.serialize(&true);
                s.serialize(x);
            }
        }
    }
    fn deserialize_self(d: &mut Deserializer) -> Self {
        if d.deserialize::<bool>() {
            Some(d.deserialize())
        } else {
            None
        }
    }
    fn deserialize_on_heap<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a>
    where
        T: 'a,
    {
        Box::new(Self::deserialize_self(d))
    }
}
impl<T: PlainOldData> PlainOldData for Option<T> {}

trait BaseTrait {}

struct BaseType;

impl BaseTrait for BaseType {}

fn extract_vtable_ptr<T: ?Sized>(metadata: &std::ptr::DynMetadata<T>) -> *const () {
    // Yeah, screw me
    unsafe { *(metadata as *const std::ptr::DynMetadata<T> as *const *const ()) }
}

fn get_base_vtable_ptr() -> *const () {
    extract_vtable_ptr(&std::ptr::metadata(&BaseType as &dyn BaseTrait))
}

impl<T: ?Sized> Object for std::ptr::DynMetadata<T> {
    fn serialize_self(&self, s: &mut Serializer) {
        s.serialize(
            &(extract_vtable_ptr(self) as usize).wrapping_sub(get_base_vtable_ptr() as usize),
        );
    }
    fn deserialize_self(d: &mut Deserializer) -> Self {
        let vtable_ptr = d
            .deserialize::<usize>()
            .wrapping_add(get_base_vtable_ptr() as usize) as *const ();
        let mut metadata: std::mem::MaybeUninit<Self> = std::mem::MaybeUninit::uninit();
        unsafe {
            *(metadata.as_mut_ptr() as *mut std::ptr::DynMetadata<T> as *mut *const ()) =
                vtable_ptr;
            metadata.assume_init()
        }
    }
    fn deserialize_on_heap<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a>
    where
        T: 'a,
    {
        Box::new(Self::deserialize_self(d))
    }
}

impl<T: Object + std::ptr::Pointee + ?Sized> Object for Box<T>
where
    T::Metadata: EfficientObject,
{
    fn serialize_self(&self, s: &mut Serializer) {
        s.serialize(&std::ptr::metadata(self.as_ref()));
        s.serialize(self.as_ref());
    }
    fn deserialize_self(d: &mut Deserializer) -> Self {
        let metadata = d.deserialize::<T::Metadata>();
        let data_ptr = unsafe {
            Box::into_raw(
                (*std::ptr::from_raw_parts::<T>(std::ptr::null(), metadata))
                    .deserialize_on_heap_efficiently(d),
            )
        };
        // Switch vtable
        let fat_ptr = std::ptr::from_raw_parts_mut(data_ptr.to_raw_parts().0, metadata);
        unsafe { Box::from_raw(fat_ptr) }
    }
    fn deserialize_on_heap<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a>
    where
        T: 'a,
    {
        Box::new(Self::deserialize_self(d))
    }
}

impl<T: 'static + Object> Object for Rc<T> {
    fn serialize_self(&self, s: &mut Serializer) {
        match s.learn_cyclic(Rc::as_ptr(self) as *const c_void) {
            None => {
                s.serialize(&0usize);
                s.serialize(&**self);
            }
            Some(id) => {
                s.serialize(&id);
            }
        }
    }
    fn deserialize_self(d: &mut Deserializer) -> Self {
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
    fn deserialize_on_heap<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a>
    where
        T: 'a,
    {
        Box::new(Self::deserialize_self(d))
    }
}

impl<T: 'static + Object> Object for Arc<T> {
    fn serialize_self(&self, s: &mut Serializer) {
        match s.learn_cyclic(Arc::as_ptr(self) as *const c_void) {
            None => {
                s.serialize(&0usize);
                s.serialize(&**self);
            }
            Some(id) => {
                s.serialize(&id);
            }
        }
    }
    fn deserialize_self(d: &mut Deserializer) -> Self {
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
    fn deserialize_on_heap<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a>
    where
        T: 'a,
    {
        Box::new(Self::deserialize_self(d))
    }
}

impl Object for std::path::PathBuf {
    fn serialize_self(&self, s: &mut Serializer) {
        // XXX: unnecessary heap usage
        s.serialize(&self.as_os_str().to_owned());
    }
    fn deserialize_self(d: &mut Deserializer) -> Self {
        d.deserialize::<std::ffi::OsString>().into()
    }
    fn deserialize_on_heap<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a> {
        Box::new(Self::deserialize_self(d))
    }
}

impl<T: Object, const N: usize> Object for [T; N] {
    fn serialize_self(&self, s: &mut Serializer) {
        s.serialize_slice(self);
    }
    fn deserialize_self(d: &mut Deserializer) -> Self {
        [0; N].map(|_| d.deserialize())
    }
    fn deserialize_on_heap<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a>
    where
        T: 'a,
    {
        Box::new(Self::deserialize_self(d))
    }
}
impl<T: PlainOldData, const N: usize> PlainOldData for [T; N] {}

impl<T: Object> Object for Vec<T> {
    fn serialize_self(&self, s: &mut Serializer) {
        s.serialize(&self.len());
        s.serialize_slice(self.as_slice())
    }
    fn deserialize_self(d: &mut Deserializer) -> Self {
        let size: usize = d.deserialize();
        let mut seq = Vec::with_capacity(size);
        for _ in 0..size {
            seq.push(d.deserialize());
        }
        seq
    }
    fn deserialize_on_heap<'serde>(&self, d: &mut Deserializer) -> Box<dyn Object + 'serde>
    where
        T: 'serde,
    {
        Box::new(Self::deserialize_self(d))
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
        impl<T: Object $(+ $tbound1 $(+ $tbound2)*)* $(, $typaram: $bound1 $(+ $bound2)*,)*> Object
            for $ty<T $(, $typaram)*>
        {
            fn serialize_self(&self, s: &mut Serializer) {
                s.serialize(&self.len());
                for item in self.iter() {
                    s.serialize(item);
                }
            }
            fn deserialize_self(d: &mut Deserializer) -> Self {
                let $size: usize = d.deserialize();
                let mut $seq = $with_capacity;
                for _ in 0..$size {
                    $push(&mut $seq, d.deserialize());
                }
                $seq
            }
            fn deserialize_on_heap<'serde>(&self, d: &mut Deserializer) -> Box<dyn Object + 'serde> where T: 'serde $(, $typaram: 'serde)* {
                Box::new(Self::deserialize_self(d))
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
        impl<
            K: Object $(+ $kbound1 $(+ $kbound2)*)*,
            V: Object
            $(, $typaram: $bound1 $(+ $bound2)*,)*
        > Object
            for $ty<K, V $(, $typaram)*>
        {
            fn serialize_self(&self, s: &mut Serializer) {
                s.serialize(&self.len());
                for (key, value) in self.iter() {
                    s.serialize(key);
                    s.serialize(value);
                }
            }
            fn deserialize_self(d: &mut Deserializer) -> Self {
                let $size: usize = d.deserialize();
                let mut map = $with_capacity;
                for _ in 0..$size {
                    map.insert(d.deserialize(), d.deserialize());
                }
                map
            }
            fn deserialize_on_heap<'serde>(&self, d: &mut Deserializer) -> Box<dyn Object + 'serde> where K: 'serde, V: 'serde $(, $typaram: 'serde)* {
                Box::new(Self::deserialize_self(d))
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

impl<T: Object, E: Object> Object for Result<T, E> {
    fn serialize_self(&self, s: &mut Serializer) {
        match self {
            Ok(ref ok) => {
                s.serialize(&true);
                s.serialize(ok);
            }
            Err(ref err) => {
                s.serialize(&false);
                s.serialize(err);
            }
        }
    }
    fn deserialize_self(d: &mut Deserializer) -> Self {
        if d.deserialize::<bool>() {
            Ok(d.deserialize())
        } else {
            Err(d.deserialize())
        }
    }
    fn deserialize_on_heap<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a>
    where
        T: 'a,
        E: 'a,
    {
        Box::new(Self::deserialize_self(d))
    }
}
impl<T: PlainOldData, E: PlainOldData> PlainOldData for Result<T, E> {}

impl Object for OwnedHandle {
    fn serialize_self(&self, s: &mut Serializer) {
        let handle = s.add_handle(self.as_raw_handle());
        s.serialize(&handle)
    }
    fn deserialize_self(d: &mut Deserializer) -> Self {
        let handle = d.deserialize();
        d.drain_handle(handle)
    }
    fn deserialize_on_heap<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a> {
        Box::new(Self::deserialize_self(d))
    }
}

impl Object for std::fs::File {
    fn serialize_self(&self, s: &mut Serializer) {
        let handle = s.add_handle(self.as_raw_handle());
        s.serialize(&handle)
    }
    fn deserialize_self(d: &mut Deserializer) -> Self {
        d.deserialize::<OwnedHandle>().into()
    }
    fn deserialize_on_heap<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a> {
        Box::new(Self::deserialize_self(d))
    }
}

#[doc(cfg(feature = "tokio"))]
#[cfg(feature = "tokio")]
impl Object for tokio::fs::File {
    fn serialize_self(&self, s: &mut Serializer) {
        let handle = s.add_handle(self.as_raw_handle());
        s.serialize(&handle)
    }
    fn deserialize_self(d: &mut Deserializer) -> Self {
        let handle = d.deserialize();
        unsafe {
            <Self as FromRawHandle>::from_raw_handle(d.drain_handle(handle).into_raw_handle())
        }
    }
    fn deserialize_on_heap<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a> {
        Box::new(Self::deserialize_self(d))
    }
}

#[doc(cfg(unix))]
#[cfg(unix)]
impl Object for std::os::unix::net::UnixStream {
    fn serialize_self(&self, s: &mut Serializer) {
        let handle = s.add_handle(self.as_raw_handle());
        s.serialize(&handle)
    }
    fn deserialize_self(d: &mut Deserializer) -> Self {
        d.deserialize::<OwnedHandle>().into()
    }
    fn deserialize_on_heap<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a> {
        Box::new(Self::deserialize_self(d))
    }
}

#[doc(cfg(all(unix, feature = "tokio")))]
#[cfg(all(unix, feature = "tokio"))]
impl Object for tokio::net::UnixStream {
    fn serialize_self(&self, s: &mut Serializer) {
        let handle = s.add_handle(self.as_raw_handle());
        s.serialize(&handle)
    }
    fn deserialize_self(d: &mut Deserializer) -> Self {
        Self::from_std(d.deserialize()).expect("Failed to deserialize tokio::net::UnixStream")
    }
    fn deserialize_on_heap<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a> {
        Box::new(Self::deserialize_self(d))
    }
}

#[doc(cfg(all(unix, feature = "tokio")))]
#[cfg(all(unix, feature = "tokio"))]
impl Object for tokio_seqpacket::UnixSeqpacket {
    fn serialize_self(&self, s: &mut Serializer) {
        let handle = s.add_handle(self.as_raw_fd());
        s.serialize(&handle)
    }
    fn deserialize_self(d: &mut Deserializer) -> Self {
        let handle = d.deserialize();
        unsafe {
            Self::from_raw_fd(d.drain_handle(handle).into_raw_handle())
                .expect("Failed to deserialize tokio_seqpacket::UnixSeqpacket")
        }
    }
    fn deserialize_on_heap<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a> {
        Box::new(Self::deserialize_self(d))
    }
}

#[doc(cfg(windows))]
#[cfg(windows)]
impl_pod!(for RawHandle);
