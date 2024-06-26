use crate::{imp::implements, Deserializer, NonTrivialObject, Serializer};
use std::io::Result;

pub trait PlainOldData: NonTrivialObject {}

mod private {
    pub trait Sealed {}
}

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
pub trait Object: private::Sealed {
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
    unsafe fn deserialize_self(d: &mut Deserializer) -> Result<Self>
    where
        Self: Sized;
    #[doc(hidden)]
    unsafe fn deserialize_on_heap(d: &mut Deserializer) -> Result<*mut ()>
    where
        Self: Sized;
    #[doc(hidden)]
    #[cfg(feature = "nightly")]
    unsafe fn deserialize_on_heap_ptr(self: *const Self, d: &mut Deserializer) -> Result<*mut ()>;
    #[doc(hidden)]
    #[cfg(not(feature = "nightly"))]
    fn deserialize_on_heap_get(&self) -> unsafe fn(&mut Deserializer) -> Result<*mut ()>;
}

impl<T: NonTrivialObject> private::Sealed for T {}
impl<T: NonTrivialObject> Object for T {
    fn serialize_self(&self, s: &mut Serializer) {
        if implements!(T: PlainOldData) {
            s.write(unsafe {
                std::slice::from_raw_parts(self as *const T as *const u8, std::mem::size_of::<T>())
            });
        } else {
            self.serialize_self_non_trivial(s);
        }
    }

    fn serialize_slice(elements: &[Self], s: &mut Serializer)
    where
        Self: Sized,
    {
        if implements!(T: PlainOldData) {
            s.write(unsafe {
                std::slice::from_raw_parts(
                    elements.as_ptr() as *const u8,
                    std::mem::size_of_val(elements),
                )
            });
        } else {
            for element in elements {
                element.serialize_self_non_trivial(s)
            }
        }
    }

    unsafe fn deserialize_self(d: &mut Deserializer) -> Result<Self>
    where
        Self: Sized,
    {
        if implements!(T: PlainOldData) {
            let mut val = std::mem::MaybeUninit::<T>::uninit();
            d.read(std::slice::from_raw_parts_mut(
                val.as_mut_ptr() as *mut u8,
                std::mem::size_of::<T>(),
            ));
            Ok(val.assume_init())
        } else {
            T::deserialize_self_non_trivial(d)
        }
    }

    unsafe fn deserialize_on_heap(d: &mut Deserializer) -> Result<*mut ()>
    where
        Self: Sized,
    {
        Ok(Box::into_raw(Box::new(Self::deserialize_self(d)?)) as *mut ())
    }

    #[cfg(feature = "nightly")]
    unsafe fn deserialize_on_heap_ptr(self: *const T, d: &mut Deserializer) -> Result<*mut ()> {
        Self::deserialize_on_heap(d)
    }
    #[cfg(not(feature = "nightly"))]
    fn deserialize_on_heap_get(&self) -> unsafe fn(&mut Deserializer) -> Result<*mut ()> {
        Self::deserialize_on_heap
    }
}
