//! Serialization and deserialization.
//!
//! This is *not* the well-known `serde` crate. We use custom serialization methods because we need
//! to serialize not only data structures, but objects with real-world side-effects, e.g. files.

use crate::handles::{BorrowedHandle, OwnedHandle};
use std::fmt;

/// Stateful serialization.
///
/// The serializer stores binary data corresponding to the serialized object and also borrowes file
/// descriptors inside the object for `'fd`.
pub struct Serializer<'fd> {
    data: Vec<u8>,
    handles: Vec<BorrowedHandle<'fd>>,
}

impl<'fd> Serializer<'fd> {
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
    /// The object is borrowed for the lifetime of the serializer so that file descriptors can be
    /// transmitted. Temporary objects that cannot be borrowed for this long can be serialized with
    /// [`serialize_temporary`](Serializer::serialize_temporary).
    pub fn serialize<T: Object>(&mut self, data: &'fd T) {
        data.serialize_self(self);
    }

    /// Append serialized data of a slice of objects, as if calling [`Serializer::serialize`] for
    /// each element.
    pub fn serialize_slice<T: Object>(&mut self, data: &'fd [T]) {
        Object::serialize_slice(data, self);
    }

    /// Append serialized data of a temporary object free of file handles, without a long borrow.
    ///
    /// Panics if the object contains file handles.
    pub fn serialize_temporary<T: Object>(&mut self, data: T) {
        let mut s1 = Serializer::new();
        core::mem::swap(&mut self.data, &mut s1.data);
        s1.serialize(&data);
        assert!(
            s1.handles.is_empty(),
            "serialize_temporary invoked with an object containing file handles"
        );
        core::mem::swap(&mut self.data, &mut s1.data);
    }

    /// Store a file handle.
    pub fn serialize_handle(&mut self, handle: BorrowedHandle<'fd>) {
        self.handles.push(handle);
    }

    /// Extract serialized data and file handles.
    pub fn into_parts(self) -> (Vec<u8>, Vec<BorrowedHandle<'fd>>) {
        (self.data, self.handles)
    }
}

impl fmt::Debug for Serializer<'_> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        // Omit internal state because it's not user-friendly.
        fmt.debug_struct("Serializer").finish()
    }
}

impl Default for Serializer<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl IntoIterator for Serializer<'_> {
    type Item = u8;
    type IntoIter = <Vec<u8> as IntoIterator>::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        self.data.into_iter()
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
    /// serializer.serialize(&1u8);
    /// serializer.serialize(&2u16);
    /// let mut deserializer = Deserializer::new(serializer.into_parts().0, Vec::new());
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
    /// serializer.serialize(&1u8);
    /// serializer.serialize(&2u16);
    /// let mut deserializer = Deserializer::new(serializer.into_parts().0, Vec::new());
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
///     fn serialize_self<'a>(&'a self, s: &mut Serializer<'a>) {
///         s.serialize(&self.first);
///         s.serialize(&self.second);
///     }
///     unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
///         let first = d.deserialize::<T>();
///         let second = d.deserialize::<U>();
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
/// automatically for [`std::fs::File`], [`OwnedHandle`] and related types, but if you have a
/// different runtime, things might get a bit complicated.
///
/// In this case, the following example should be of help:
///
/// ```rust
/// use crossmist::{handles::{AsHandle, OwnedHandle}, Deserializer, Object, Serializer};
/// use std::fs::File;
/// use std::io::Result;
///
/// struct CustomFile(std::fs::File);
///
/// unsafe impl Object for CustomFile {
///     fn serialize_self<'a>(&'a self, s: &mut Serializer<'a>) {
///         // serialize_handle adds the handle (fd)
///         s.serialize_handle(self.0.as_handle());
///     }
///     unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
///         // Deserializing OwnedHandle results in the ID being resolved into the handle, which can
///         // then be used to create the instance of the object we are deserializing
///         Self(d.deserialize::<OwnedHandle>().into())
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
    fn serialize_self<'a>(&'a self, s: &mut Serializer<'a>);
    /// Serialize an array of objects into a serializer.
    fn serialize_slice<'a>(elements: &'a [Self], s: &mut Serializer<'a>)
    where
        Self: Sized,
    {
        for element in elements {
            element.serialize_self(s);
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
    #[cfg(feature = "nightly")]
    unsafe fn deserialize_on_heap_ptr(self: *const Self, d: &mut Deserializer) -> *mut ();
    #[cfg(not(feature = "nightly"))]
    fn deserialize_on_heap_get(&self) -> unsafe fn(&mut Deserializer) -> *mut ();
}

impl<T: Object> BaseObject for T {
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
