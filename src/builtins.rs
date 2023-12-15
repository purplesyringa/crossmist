#[cfg(windows)]
use crate::handles::RawHandle;
#[cfg(feature = "tokio")]
use crate::handles::{FromRawHandle, IntoRawHandle};
use crate::{
    handles::{AsRawHandle, OwnedHandle},
    pod::PlainOldData,
    Deserializer, NonTrivialObject, Object, Serializer,
};
use paste::paste;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, LinkedList, VecDeque};
use std::hash::{BuildHasher, Hash};
use std::os::raw::c_void;
use std::rc::Rc;
use std::sync::Arc;

macro_rules! impl_pod {
    ([$($generics:tt)*] for $t:ty) => {
        impl<$($generics)*> NonTrivialObject for $t {
            fn serialize_self_non_trivial(&self, _s: &mut Serializer) {
                unreachable!()
            }
            unsafe fn deserialize_self_non_trivial(_d: &mut Deserializer) -> Self {
                unreachable!()
            }
            unsafe fn deserialize_on_heap_non_trivial<'a>(&self, _d: &mut Deserializer) -> Box<dyn Object + 'a> {
                unreachable!()
            }
        }
        impl<$($generics)*> PlainOldData for $t {}
    };
    (for $t:ty) => {
        impl NonTrivialObject for $t {
            fn serialize_self_non_trivial(&self, _s: &mut Serializer) {
                unreachable!()
            }
            unsafe fn deserialize_self_non_trivial(_d: &mut Deserializer) -> Self {
                unreachable!()
            }
            unsafe fn deserialize_on_heap_non_trivial<'a>(&self, _d: &mut Deserializer) -> Box<dyn Object + 'a> {
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
impl_pod!(for std::time::Instant);
impl_pod!(for std::time::SystemTime);

impl NonTrivialObject for String {
    fn serialize_self_non_trivial(&self, s: &mut Serializer) {
        s.serialize(&self.len());
        s.serialize_slice(self.as_bytes());
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Self {
        unsafe { String::from_utf8_unchecked(d.deserialize::<Vec<u8>>()) }
    }
    unsafe fn deserialize_on_heap_non_trivial<'a>(
        &self,
        d: &mut Deserializer,
    ) -> Box<dyn Object + 'a> {
        Box::new(Self::deserialize_self_non_trivial(d))
    }
}

impl NonTrivialObject for std::ffi::CString {
    fn serialize_self_non_trivial(&self, s: &mut Serializer) {
        let bytes = self.as_bytes();
        s.serialize(&bytes.len());
        s.serialize_slice(bytes);
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Self {
        unsafe { Self::from_vec_unchecked(d.deserialize::<Vec<u8>>()) }
    }
    unsafe fn deserialize_on_heap_non_trivial<'a>(
        &self,
        d: &mut Deserializer,
    ) -> Box<dyn Object + 'a> {
        Box::new(Self::deserialize_self_non_trivial(d))
    }
}

impl NonTrivialObject for std::ffi::OsString {
    fn serialize_self_non_trivial(&self, s: &mut Serializer) {
        let bytes = self.as_encoded_bytes();
        s.serialize(&bytes.len());
        s.serialize_slice(bytes);
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Self {
        unsafe { Self::from_encoded_bytes_unchecked(d.deserialize()) }
    }
    unsafe fn deserialize_on_heap_non_trivial<'a>(
        &self,
        d: &mut Deserializer,
    ) -> Box<dyn Object + 'a> {
        Box::new(Self::deserialize_self_non_trivial(d))
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
            impl<$([<T $tail>]: Object),*> NonTrivialObject for ($([<T $tail>],)*) {
                #[allow(unused_variables)]
                fn serialize_self_non_trivial(&self, s: &mut Serializer) {
                    serialize_rev!(s, self, $($tail)*);
                }
                #[allow(unused_variables)]
                unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Self {
                    $( let [<x $tail>] = d.deserialize(); )*
                    ($([<x $tail>],)*)
                }
                unsafe fn deserialize_on_heap_non_trivial<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a>
                where
                    $([<T $tail>]: 'a),*
                {
                    Box::new(Self::deserialize_self_non_trivial(d))
                }
            }
            impl<$([<T $tail>]: PlainOldData),*> PlainOldData for ($([<T $tail>],)*) {}
        }
    }
}

impl_serialize_for_tuple!(x 19 18 17 16 15 14 13 12 11 10 9 8 7 6 5 4 3 2 1 0);

impl<T: Object> NonTrivialObject for Option<T> {
    fn serialize_self_non_trivial(&self, s: &mut Serializer) {
        match self {
            None => s.serialize(&false),
            Some(ref x) => {
                s.serialize(&true);
                s.serialize(x);
            }
        }
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Self {
        if d.deserialize::<bool>() {
            Some(d.deserialize())
        } else {
            None
        }
    }
    unsafe fn deserialize_on_heap_non_trivial<'a>(
        &self,
        d: &mut Deserializer,
    ) -> Box<dyn Object + 'a>
    where
        T: 'a,
    {
        Box::new(Self::deserialize_self_non_trivial(d))
    }
}
impl<T: PlainOldData> PlainOldData for Option<T> {}

trait BaseTrait {}

struct BaseType;

impl BaseTrait for BaseType {}

// This needs to be a singleton to prevent different codegen units from using different copies of a
// single vtable for BaseType. See also: https://github.com/alecmocatta/relative/pull/2
static BASE_OBJECT: &(dyn BaseTrait + Sync) = &BaseType;

fn extract_vtable_ptr<T: ?Sized>(metadata: &std::ptr::DynMetadata<T>) -> *const () {
    // Yeah, screw me
    unsafe { *(metadata as *const std::ptr::DynMetadata<T> as *const *const ()) }
}

fn get_base_vtable_ptr() -> *const () {
    extract_vtable_ptr(&std::ptr::metadata(BASE_OBJECT))
}

impl<T: ?Sized> NonTrivialObject for std::ptr::DynMetadata<T> {
    fn serialize_self_non_trivial(&self, s: &mut Serializer) {
        s.serialize(
            &(extract_vtable_ptr(self) as usize).wrapping_sub(get_base_vtable_ptr() as usize),
        );
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Self {
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
    unsafe fn deserialize_on_heap_non_trivial<'a>(
        &self,
        d: &mut Deserializer,
    ) -> Box<dyn Object + 'a>
    where
        T: 'a,
    {
        Box::new(Self::deserialize_self_non_trivial(d))
    }
}

impl<T: Object + std::ptr::Pointee + ?Sized> NonTrivialObject for Box<T>
where
    T::Metadata: Object,
{
    fn serialize_self_non_trivial(&self, s: &mut Serializer) {
        s.serialize(&std::ptr::metadata(self.as_ref()));
        s.serialize(self.as_ref());
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Self {
        let metadata = d.deserialize::<T::Metadata>();
        let data_ptr = unsafe {
            Box::into_raw(
                (*std::ptr::from_raw_parts::<T>(std::ptr::null(), metadata)).deserialize_on_heap(d),
            )
        };
        // Switch vtable
        let fat_ptr = std::ptr::from_raw_parts_mut(data_ptr.to_raw_parts().0, metadata);
        unsafe { Box::from_raw(fat_ptr) }
    }
    unsafe fn deserialize_on_heap_non_trivial<'a>(
        &self,
        d: &mut Deserializer,
    ) -> Box<dyn Object + 'a>
    where
        T: 'a,
    {
        Box::new(Self::deserialize_self_non_trivial(d))
    }
}

impl<T: 'static + Object> NonTrivialObject for Rc<T> {
    fn serialize_self_non_trivial(&self, s: &mut Serializer) {
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
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Self {
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
    unsafe fn deserialize_on_heap_non_trivial<'a>(
        &self,
        d: &mut Deserializer,
    ) -> Box<dyn Object + 'a>
    where
        T: 'a,
    {
        Box::new(Self::deserialize_self_non_trivial(d))
    }
}

impl<T: 'static + Object> NonTrivialObject for Arc<T> {
    fn serialize_self_non_trivial(&self, s: &mut Serializer) {
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
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Self {
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
    unsafe fn deserialize_on_heap_non_trivial<'a>(
        &self,
        d: &mut Deserializer,
    ) -> Box<dyn Object + 'a>
    where
        T: 'a,
    {
        Box::new(Self::deserialize_self_non_trivial(d))
    }
}

impl NonTrivialObject for std::path::PathBuf {
    fn serialize_self_non_trivial(&self, s: &mut Serializer) {
        let bytes = self.as_os_str().as_encoded_bytes();
        s.serialize(&bytes.len());
        s.serialize_slice(bytes);
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Self {
        d.deserialize::<std::ffi::OsString>().into()
    }
    unsafe fn deserialize_on_heap_non_trivial<'a>(
        &self,
        d: &mut Deserializer,
    ) -> Box<dyn Object + 'a> {
        Box::new(Self::deserialize_self_non_trivial(d))
    }
}

impl<T: Object, const N: usize> NonTrivialObject for [T; N] {
    fn serialize_self_non_trivial(&self, s: &mut Serializer) {
        s.serialize_slice(self);
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Self {
        [0; N].map(|_| d.deserialize())
    }
    unsafe fn deserialize_on_heap_non_trivial<'a>(
        &self,
        d: &mut Deserializer,
    ) -> Box<dyn Object + 'a>
    where
        T: 'a,
    {
        Box::new(Self::deserialize_self_non_trivial(d))
    }
}
impl<T: PlainOldData, const N: usize> PlainOldData for [T; N] {}

impl<T: Object> NonTrivialObject for Vec<T> {
    fn serialize_self_non_trivial(&self, s: &mut Serializer) {
        s.serialize(&self.len());
        s.serialize_slice(self.as_slice())
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Self {
        let size: usize = d.deserialize();
        let mut seq = Vec::with_capacity(size);
        for _ in 0..size {
            seq.push(d.deserialize());
        }
        seq
    }
    unsafe fn deserialize_on_heap_non_trivial<'a>(
        &self,
        d: &mut Deserializer,
    ) -> Box<dyn Object + 'a>
    where
        T: 'a,
    {
        Box::new(Self::deserialize_self_non_trivial(d))
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
        impl<T: Object $(+ $tbound1 $(+ $tbound2)*)* $(, $typaram: $bound1 $(+ $bound2)*,)*> NonTrivialObject
            for $ty<T $(, $typaram)*>
        {
            fn serialize_self_non_trivial(&self, s: &mut Serializer) {
                s.serialize(&self.len());
                for item in self.iter() {
                    s.serialize(item);
                }
            }
            unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Self {
                let $size: usize = d.deserialize();
                let mut $seq = $with_capacity;
                for _ in 0..$size {
                    $push(&mut $seq, d.deserialize());
                }
                $seq
            }
            unsafe fn deserialize_on_heap_non_trivial<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a> where T: 'a $(, $typaram: 'a)* {
                Box::new(Self::deserialize_self_non_trivial(d))
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
        > NonTrivialObject
            for $ty<K, V $(, $typaram)*>
        {
            fn serialize_self_non_trivial(&self, s: &mut Serializer) {
                s.serialize(&self.len());
                for (key, value) in self.iter() {
                    s.serialize(key);
                    s.serialize(value);
                }
            }
            unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Self {
                let $size: usize = d.deserialize();
                let mut map = $with_capacity;
                for _ in 0..$size {
                    map.insert(d.deserialize(), d.deserialize());
                }
                map
            }
            unsafe fn deserialize_on_heap_non_trivial<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a> where K: 'a, V: 'a $(, $typaram: 'a)* {
                Box::new(Self::deserialize_self_non_trivial(d))
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

impl<T: Object, E: Object> NonTrivialObject for Result<T, E> {
    fn serialize_self_non_trivial(&self, s: &mut Serializer) {
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
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Self {
        if d.deserialize::<bool>() {
            Ok(d.deserialize())
        } else {
            Err(d.deserialize())
        }
    }
    unsafe fn deserialize_on_heap_non_trivial<'a>(
        &self,
        d: &mut Deserializer,
    ) -> Box<dyn Object + 'a>
    where
        T: 'a,
        E: 'a,
    {
        Box::new(Self::deserialize_self_non_trivial(d))
    }
}
impl<T: PlainOldData, E: PlainOldData> PlainOldData for Result<T, E> {}

impl NonTrivialObject for OwnedHandle {
    fn serialize_self_non_trivial(&self, s: &mut Serializer) {
        let handle = s.add_handle(self.as_raw_handle());
        s.serialize(&handle)
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Self {
        let handle = d.deserialize();
        d.drain_handle(handle)
    }
    unsafe fn deserialize_on_heap_non_trivial<'a>(
        &self,
        d: &mut Deserializer,
    ) -> Box<dyn Object + 'a> {
        Box::new(Self::deserialize_self_non_trivial(d))
    }
}

impl NonTrivialObject for std::fs::File {
    fn serialize_self_non_trivial(&self, s: &mut Serializer) {
        let handle = s.add_handle(self.as_raw_handle());
        s.serialize(&handle)
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Self {
        d.deserialize::<OwnedHandle>().into()
    }
    unsafe fn deserialize_on_heap_non_trivial<'a>(
        &self,
        d: &mut Deserializer,
    ) -> Box<dyn Object + 'a> {
        Box::new(Self::deserialize_self_non_trivial(d))
    }
}

#[doc(cfg(feature = "tokio"))]
#[cfg(feature = "tokio")]
impl NonTrivialObject for tokio::fs::File {
    fn serialize_self_non_trivial(&self, s: &mut Serializer) {
        let handle = s.add_handle(self.as_raw_handle());
        s.serialize(&handle)
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Self {
        let handle = d.deserialize();
        unsafe {
            <Self as FromRawHandle>::from_raw_handle(d.drain_handle(handle).into_raw_handle())
        }
    }
    unsafe fn deserialize_on_heap_non_trivial<'a>(
        &self,
        d: &mut Deserializer,
    ) -> Box<dyn Object + 'a> {
        Box::new(Self::deserialize_self_non_trivial(d))
    }
}

#[doc(cfg(unix))]
#[cfg(unix)]
impl NonTrivialObject for std::os::unix::net::UnixStream {
    fn serialize_self_non_trivial(&self, s: &mut Serializer) {
        let handle = s.add_handle(self.as_raw_handle());
        s.serialize(&handle)
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Self {
        d.deserialize::<OwnedHandle>().into()
    }
    unsafe fn deserialize_on_heap_non_trivial<'a>(
        &self,
        d: &mut Deserializer,
    ) -> Box<dyn Object + 'a> {
        Box::new(Self::deserialize_self_non_trivial(d))
    }
}

#[doc(cfg(all(unix, feature = "tokio")))]
#[cfg(all(unix, feature = "tokio"))]
impl NonTrivialObject for tokio::net::UnixStream {
    fn serialize_self_non_trivial(&self, s: &mut Serializer) {
        let handle = s.add_handle(self.as_raw_handle());
        s.serialize(&handle)
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Self {
        Self::from_std(d.deserialize()).expect("Failed to deserialize tokio::net::UnixStream")
    }
    unsafe fn deserialize_on_heap_non_trivial<'a>(
        &self,
        d: &mut Deserializer,
    ) -> Box<dyn Object + 'a> {
        Box::new(Self::deserialize_self_non_trivial(d))
    }
}

#[doc(cfg(all(unix, feature = "tokio")))]
#[cfg(all(unix, feature = "tokio"))]
impl NonTrivialObject for tokio_seqpacket::UnixSeqpacket {
    fn serialize_self_non_trivial(&self, s: &mut Serializer) {
        let handle = s.add_handle(self.as_raw_fd());
        s.serialize(&handle)
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Self {
        let handle = d.deserialize();
        unsafe {
            Self::from_raw_fd(d.drain_handle(handle).into_raw_handle())
                .expect("Failed to deserialize tokio_seqpacket::UnixSeqpacket")
        }
    }
    unsafe fn deserialize_on_heap_non_trivial<'a>(
        &self,
        d: &mut Deserializer,
    ) -> Box<dyn Object + 'a> {
        Box::new(Self::deserialize_self_non_trivial(d))
    }
}

#[doc(cfg(windows))]
#[cfg(windows)]
impl_pod!(for RawHandle);
