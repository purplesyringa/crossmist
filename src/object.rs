use crate::{
    owning_ref::OwningRef,
    serde::{Deserializer, Serializer},
};

/// A value that can be sent between processes.
///
/// This trait is implemented for most types from the standard library for which it can reasonably
/// be implemented, and if you need it for your structs and enums, you can use
/// [`#[derive(Object)]`](derive@crate::Object).
///
/// You don't need to call the methods of this trait directly: crossmist does this for you when you
/// send values over channels. This trait is not suited as a general serialization mechanism and
/// should be used only for crossmist-based cross-process communication.
///
/// # Custom implementations
///
/// If you have a type for which `#[derive(Object)]` does not produce the desired semantics, e.g. if
/// you have additional state stored elsewhere that should be dumped in the serialization stream,
/// implement this trait based on the following template:
///
/// ```rust
/// use crossmist::{Object, serde::{Deserializer, Serializer}};
/// use std::io::Result;
///
/// struct SimplePair<T: Object, U: Object> {
///     first: T,
///     second: U,
/// }
///
/// unsafe impl<T: Object, U: Object> Object for SimplePair<T, U> {
///     fn serialize_self(self, s: &mut Serializer) {
///         s.serialize(self.first);
///         s.serialize(self.second);
///     }
///     unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
///         let first = unsafe { d.deserialize::<T>() };
///         let second = unsafe { d.deserialize::<U>() };
///         Self { first, second }
///     }
/// }
/// ```
///
/// Objects referencing file descriptors (on Unix) or handles/sockets (on Windows) can be serialized
/// by converting to [`OwnedFd`](std::os::unix::io::OwnedFd) or
/// [`OwnedHandle`](std::os::windows::io::OwnedHandle)/[`OwnedSocket`](std::os::windows::io::OwnedSocket),
/// for example:
///
/// ```rust
/// use crossmist::{Object, serde::{Deserializer, Serializer}};
/// use std::fs::File;
/// use std::io::Result;
/// #[cfg(unix)]
/// use std::os::unix::io::OwnedFd as Resource;
/// #[cfg(windows)]
/// use std::os::windows::io::OwnedHandle as Resource;
///
/// struct CustomFile(std::fs::File);
///
/// unsafe impl Object for CustomFile {
///     fn serialize_self(self, s: &mut Serializer) {
///         s.serialize(Resource::from(self.0));
///     }
///     unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
///         Self(unsafe { d.deserialize::<Resource>() }.into())
///     }
/// }
/// ```
///
///
/// # Safety
///
/// An implementation of this trait is safe if the order of types used during serialization and
/// deserialization matches. See the documentation of [`Deserializer::deserialize`] for details.
#[allow(private_bounds)]
pub unsafe trait Object: BaseObject {
    /// Serialize a single object.
    fn serialize_self(self, s: &mut Serializer);
    /// Serialize an array of objects into a serializer.
    ///
    /// Not part of the stable API.
    #[doc(hidden)]
    fn serialize_slice(elements: OwningRef<'_, [Self]>, s: &mut Serializer)
    where
        Self: Sized,
    {
        for element in elements {
            s.serialize(element);
        }
    }
    /// Deserialize a single object.
    ///
    /// # Safety
    ///
    /// This function is safe to call if the value at the current position in the serialized stream
    /// was produced by calling [`serialize_self`](Self::serialize_self) on an instance of the same
    /// type. See the documentation of [`Deserializer::deserialize`] for details.
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self
    where
        Self: Sized;
}

// These methods need to be dyn-compatible, but can only be implemented for `Self: Sized`, so they
// can't go into the `Object` trait as a default implementation directly. Instead they're
// blanket-implemented in a supertrait.
pub(crate) trait BaseObject {
    // We can't use `*mut Self` or `OwningRef<'_, Self>` as the receiver, so we cast it to
    // `&mut self` on the caller side and then cast it back to `OwningRef` in the implementation.
    #[doc(hidden)]
    unsafe fn serialize_taking(&mut self, s: &mut Serializer);
    #[cfg(feature = "nightly")]
    #[doc(hidden)]
    unsafe fn deserialize_on_heap_ptr(self: *const Self, d: &mut Deserializer) -> *mut ();
    #[cfg(not(feature = "nightly"))]
    #[doc(hidden)]
    fn deserialize_on_heap_get(&self) -> unsafe fn(&mut Deserializer) -> *mut ();
}

impl<T: Object> BaseObject for T {
    unsafe fn serialize_taking(&mut self, s: &mut Serializer) {
        unsafe { OwningRef::from_leaked(self) }
            .take()
            .serialize_self(s);
    }
    #[cfg(feature = "nightly")]
    unsafe fn deserialize_on_heap_ptr(self: *const Self, d: &mut Deserializer) -> *mut () {
        unsafe { deserialize_on_heap::<T>(d) }
    }
    #[cfg(not(feature = "nightly"))]
    fn deserialize_on_heap_get(&self) -> unsafe fn(&mut Deserializer) -> *mut () {
        deserialize_on_heap::<T>
    }
}

unsafe fn deserialize_on_heap<T: Object>(d: &mut Deserializer) -> *mut () {
    Box::into_raw(Box::new(unsafe { T::deserialize_self(d) })).cast()
}
