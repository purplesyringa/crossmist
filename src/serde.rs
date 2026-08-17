//! Serialization and deserialization.
//!
//! This is *not* the well-known `serde` crate. We use custom serialization methods because we need
//! to serialize not only data structures, but objects with real-world side-effects, e.g. files.

use crate::{handles::OwnedHandle, owning_ref::OwningRef};
use std::fmt;

/// Stateful serialization.
///
/// The serializer stores binary data corresponding to the serialized object and file descriptors
/// inside it.
pub struct Serializer {
    data: Vec<u8>,
    pub(crate) handles: Vec<OwnedHandle>,
}

impl Serializer {
    /// Create a new serializer.
    pub fn new() -> Self {
        Serializer {
            data: Vec::new(),
            handles: Vec::new(),
        }
    }

    /// Append chunk of serialize data.
    pub fn write(&mut self, data: &[u8]) {
        self.data.extend_from_slice(data);
    }

    /// Append serialized data of an object.
    ///
    /// The object is consumed so that unique resources owned by the object (e.g. file descriptors)
    /// are not duplicated.
    pub fn serialize<T: Object>(&mut self, data: T) {
        data.serialize_self(self);
    }

    /// Append serialized data of an object taken by an owning reference to support unsized types.
    pub(crate) fn serialize_ref<T: Object + ?Sized>(&mut self, data: OwningRef<'_, T>) {
        unsafe { data.leak().serialize_taking(self) };
    }

    /// Append serialized data of a slice of objects, as if calling [`Serializer::serialize`] for
    /// each element, but more optimized.
    pub(crate) fn serialize_slice<T: Object>(&mut self, data: OwningRef<'_, [T]>) {
        Object::serialize_slice(data, self);
    }

    /// Extract serialized data and file handles.
    pub fn into_parts(self) -> (Vec<u8>, Vec<OwnedHandle>) {
        (self.data, self.handles)
    }
}

impl fmt::Debug for Serializer {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        // Omit internal state because it's not user-friendly.
        fmt.debug_struct("Serializer").finish()
    }
}

impl Default for Serializer {
    fn default() -> Self {
        Self::new()
    }
}

/// Stateful deserialization.
pub struct Deserializer {
    data: Vec<u8>,
    pub(crate) handles: std::vec::IntoIter<OwnedHandle>,
    pos: usize,
}

impl Deserializer {
    /// Start deserializing data obtain from a [`Serializer`].
    pub fn new(data: Vec<u8>, handles: Vec<OwnedHandle>) -> Self {
        Deserializer {
            data,
            handles: handles.into_iter(),
            pos: 0,
        }
    }

    /// Read the next `count` bytes.
    pub fn read(&mut self, count: usize) -> &[u8] {
        self.pos += count;
        &self.data[self.pos - count..self.pos]
    }

    /// Deserialize an object of a given type from `self`.
    ///
    /// Note that the deserializer is not safe to call on untrusted or corrupted data. This function
    /// returns an error if converting parsed data to Rust data structures fails, e.g. on allocation
    /// failures or when OS limits are exceeded.
    ///
    /// # Safety
    ///
    /// This function is safe to call if the order of serialized types during serialization and
    /// deserialization matches.
    ///
    /// Correct:
    ///
    /// ```
    /// use crossmist::{Deserializer, Serializer};
    ///
    /// let mut serializer = Serializer::new();
    /// serializer.serialize(1u8);
    /// serializer.serialize(2u16);
    /// let (data, handles) = serializer.into_parts();
    /// let mut deserializer = Deserializer::new(data, handles);
    /// unsafe {
    ///     assert_eq!(deserializer.deserialize::<u8>(), 1);
    ///     assert_eq!(deserializer.deserialize::<u16>(), 2);
    /// }
    /// ```
    ///
    /// Incorrect:
    ///
    /// ```no_run
    /// use crossmist::{Deserializer, Serializer};
    ///
    /// let mut serializer = Serializer::new();
    /// serializer.serialize(1u8);
    /// serializer.serialize(2u16);
    /// let (data, handles) = serializer.into_parts();
    /// let mut deserializer = Deserializer::new(data, handles);
    /// unsafe {
    ///     deserializer.deserialize::<u16>();
    ///     deserializer.deserialize::<u8>();
    /// }
    /// ```
    ///
    /// It is also sometimes safe to invoke deserialize with mismatched types if the two types have
    /// the exact same layout in crossmist's serde (not in Rust memory model!). For example,
    /// [`std::fs::File`] and [`crossmist::handles::OwnedHandle`] are compatible.
    pub unsafe fn deserialize<T: Object>(&mut self) -> T {
        unsafe { T::deserialize_self(self) }
    }

    #[cfg(windows)]
    pub(crate) fn get_rest(&self) -> &[u8] {
        &self.data[self.pos..]
    }
}

impl fmt::Debug for Deserializer {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        // Omit internal state because it's not user-friendly.
        fmt.debug_struct("Deserializer").finish()
    }
}

/// A serializable object.
///
/// This trait is already implemented for most types from the standard library for which it can
/// reasonably be implemented, and if you need it for your structs and enums, you can use
/// `#[derive(Object)]`.
///
/// You don't need to call the methods of this trait directly: crossmist does this for you whenever
/// you pass objects over channels. In case you need to transmit data via other ways of
/// communication, use [`Serializer`] and [`Deserializer`] APIs.
///
/// If you have a type for which `#[derive(Object)]` does not produce the desired semantics (e.g.
/// you have additional state stored elsewhere that should be dumped in the serialization stream),
/// implement this trait based on this template:
///
/// ```rust
/// use crossmist::{Deserializer, Object, Serializer};
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
/// Note that DSTs cannot be objects (but `Box<dyn Trait>` and `Box<[T]>` are fine).
///
///
/// # File descriptors
///
/// Sometimes, you might need to serialize objects that store references to files. This is done
/// automatically for [`std::fs::File`] and a couple other types, but if necessary, you can use
/// [`OwnedHandle`] to represent a file descriptor or a Windows handle in a cross-platform manner.
///
/// The following example should be of help:
///
/// ```rust
/// use crossmist::{handles::{IntoRawHandle, FromRawHandle, OwnedHandle}, Deserializer, Object, Serializer};
/// use std::fs::File;
/// use std::io::Result;
///
/// struct CustomFile(std::fs::File);
///
/// unsafe impl Object for CustomFile {
///     fn serialize_self(self, s: &mut Serializer) {
///         s.serialize(unsafe { OwnedHandle::from_raw_handle(self.0.into_raw_handle()) });
///     }
///     unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
///         Self(unsafe { d.deserialize::<OwnedHandle>() }.into())
///     }
/// }
/// ```
///
///
/// # Safety
///
/// An implementation of this trait function is safe if the order of serialized types during
/// serialization and deserialization matches, up to serialization layout. See the documentation of
/// [`Deserializer::deserialize`] for more details.
#[allow(private_bounds)]
pub unsafe trait Object: BaseObject {
    /// Serialize a single object into a serializer.
    fn serialize_self(self, s: &mut Serializer);
    /// Serialize an array of objects into a serializer.
    #[doc(hidden)]
    fn serialize_slice(elements: OwningRef<'_, [Self]>, s: &mut Serializer)
    where
        Self: Sized,
    {
        for element in elements {
            s.serialize(element);
        }
    }
    /// Deserialize a single object from a deserializer.
    ///
    /// This function may assume the input data is produced by [`Self::serialize_self`].
    ///
    /// # Safety
    ///
    /// This function is safe to call if the order of serialized types during serialization and
    /// deserialization matches, up to serialization layout. See the documentation of
    /// [`Deserializer::deserialize`] for more details.
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self
    where
        Self: Sized;
}

// These methods need to be dyn-compatible, but can only be implemented for `Self: Sized`, so they
// can't go into the `Object` trait as a default implementation directly. Instead they're
// blanket-implemented in a supertrait.
pub(crate) trait BaseObject {
    // We can't use `*mut Self` or `OwningRef<'_, Self>` as the receiver, so hack around.
    unsafe fn serialize_taking(&mut self, s: &mut Serializer);
    #[cfg(feature = "nightly")]
    unsafe fn deserialize_on_heap_ptr(self: *const Self, d: &mut Deserializer) -> *mut ();
    #[cfg(not(feature = "nightly"))]
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
