use crate::{relocation::RelocatablePtr, Deserializer, NonTrivialObject, Object, Serializer};
use std::ptr::{DynMetadata, Pointee};

trait BoxMetadata<T: ?Sized> {
    fn serialize_metadata(self, s: &mut Serializer);
    unsafe fn deserialize(d: &mut Deserializer) -> Box<T>;
}

impl<T: Object> BoxMetadata<T> for () {
    fn serialize_metadata(self, _s: &mut Serializer) {}
    unsafe fn deserialize(d: &mut Deserializer) -> Box<T> {
        Box::new(d.deserialize())
    }
}

impl<T: Object + Pointee<Metadata = Self> + ?Sized> BoxMetadata<T> for DynMetadata<T> {
    fn serialize_metadata(self, s: &mut Serializer) {
        s.serialize(&RelocatablePtr(unsafe {
            std::mem::transmute::<Self, *const ()>(self)
        }));
    }
    unsafe fn deserialize(d: &mut Deserializer) -> Box<T> {
        let meta = std::ptr::from_raw_parts::<T>(
            std::ptr::null(),
            std::mem::transmute::<RelocatablePtr<()>, Self>(d.deserialize()),
        );
        unsafe { Box::from_raw(Box::into_raw(meta.deserialize_on_heap(d)).with_metadata_of(meta)) }
    }
}

impl<T: Object + ?Sized> NonTrivialObject for Box<T>
where
    <T as Pointee>::Metadata: BoxMetadata<T>,
{
    fn serialize_self_non_trivial(&self, s: &mut Serializer) {
        std::ptr::metadata(self.as_ref()).serialize_metadata(s);
        self.as_ref().serialize_self(s);
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Self {
        <T as Pointee>::Metadata::deserialize(d)
    }
}
