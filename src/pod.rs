use crate::{Deserializer, NonTrivialObject, Serializer};

pub trait PlainOldData: NonTrivialObject {}

/// A serializable object.
///
/// This trait is already implemented for most types from the standard library for which it can
/// reasonably be implemented, and if you need it for your structs and enums, you can use
/// `#[derive(Object)]`.
///
/// If you need a custom implementation that `#[derive(Object)]` doesn't cover (e.g.: a library type
/// crossmist has no information about), implement [`NonTrivialObject`]. [`Object`] will be
/// implemented automatically in this case.
///
/// You don't need to call the methods of this trait directly: crossmist does this for you whenever
/// you pass objects over channels. In case you need to transmit data via other ways of
/// communication, use [`Serializer`] and [`Deserializer`] APIs.
pub trait Object: NonTrivialObject {
    /// Serialize a single object into a serializer.
    fn serialize_self(&self, s: &mut Serializer);
    /// Serialize an array of objects into a serializer.
    fn serialize_slice(elements: &[Self], s: &mut Serializer)
    where
        Self: Sized;
    /// Deserialize a single object from a deserializer.
    ///
    /// # Safety
    ///
    /// This function is safe to call if the order of serialized types during serialization and
    /// deserialization matches, up to serialization layout. See the documentation of
    /// [`Deserializer::deserialize`] for more details.
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self
    where
        Self: Sized;
    /// Deserialize a single object onto heap with dynamic typing from a deserializer.
    ///
    /// # Safety
    ///
    /// This function is safe to call if the order of serialized types during serialization and
    /// deserialization matches, up to serialization layout. See the documentation of
    /// [`Deserializer::deserialize`] for more details.
    unsafe fn deserialize_on_heap<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a>
    where
        Self: 'a;
}

impl<T: NonTrivialObject + ?Sized> Object for T {
    default fn serialize_self(&self, s: &mut Serializer) {
        self.serialize_self_non_trivial(s);
    }
    default fn serialize_slice(elements: &[Self], s: &mut Serializer)
    where
        Self: Sized,
    {
        for element in elements {
            element.serialize_self_non_trivial(s)
        }
    }
    default unsafe fn deserialize_self(d: &mut Deserializer) -> Self
    where
        Self: Sized,
    {
        T::deserialize_self_non_trivial(d)
    }
    default unsafe fn deserialize_on_heap<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a>
    where
        Self: 'a,
    {
        self.deserialize_on_heap_non_trivial(d)
    }
}

impl<T: PlainOldData> Object for T {
    fn serialize_self(&self, s: &mut Serializer) {
        s.write(unsafe {
            std::slice::from_raw_parts(self as *const T as *const u8, std::mem::size_of::<T>())
        });
    }
    fn serialize_slice(elements: &[T], s: &mut Serializer) {
        s.write(unsafe {
            std::slice::from_raw_parts(
                elements.as_ptr() as *const u8,
                std::mem::size_of::<T>() * elements.len(),
            )
        });
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        unsafe {
            let mut val = std::mem::MaybeUninit::<T>::uninit();
            d.read(std::slice::from_raw_parts_mut(
                val.as_mut_ptr() as *mut u8,
                std::mem::size_of::<T>(),
            ));
            val.assume_init()
        }
    }
    unsafe fn deserialize_on_heap<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a>
    where
        Self: 'a,
    {
        Box::new(Self::deserialize_self(d))
    }
}
