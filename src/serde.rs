//! Serialization and deserialization.
//!
//! This is *not* the well-known `serde` crate. We use custom serialization methods because we need
//! to serialize not only data structures, but objects with real-world side-effects, e.g. files.

use crate::owning_ref::OwningRef;
use std::fmt;
#[cfg(unix)]
use std::os::unix::io::OwnedFd;
#[cfg(windows)]
use std::os::windows::io::{OwnedHandle, OwnedSocket};

/// Stateful serialization.
///
/// The serializer stores binary data corresponding to the serialized object, and file descriptors
/// or other OS resources inside it.
pub struct Serializer {
    pub(crate) data: Vec<u8>,
    #[cfg(unix)]
    pub(crate) fds: Vec<OwnedFd>,
    #[cfg(windows)]
    pub(crate) handles: Vec<OwnedHandle>,
    #[cfg(windows)]
    pub(crate) sockets: Vec<OwnedSocket>,
}

impl Serializer {
    /// Create a new serializer.
    pub fn new() -> Self {
        Serializer {
            data: Vec::new(),
            #[cfg(unix)]
            fds: Vec::new(),
            #[cfg(windows)]
            handles: Vec::new(),
            #[cfg(windows)]
            sockets: Vec::new(),
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
    #[cfg(unix)]
    pub(crate) fds: std::vec::IntoIter<OwnedFd>,
    #[cfg(windows)]
    pub(crate) handles: std::vec::IntoIter<OwnedHandle>,
    #[cfg(windows)]
    pub(crate) sockets: std::vec::IntoIter<OwnedSocket>,
    pos: usize,
}

impl Deserializer {
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
    /// let mut deserializer = Deserializer::from(serializer);
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
    /// let mut deserializer = Deserializer::from(serializer);
    /// unsafe {
    ///     deserializer.deserialize::<u16>();
    ///     deserializer.deserialize::<u8>();
    /// }
    /// ```
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

impl From<Serializer> for Deserializer {
    fn from(serializer: Serializer) -> Self {
        Deserializer {
            data: serializer.data,
            #[cfg(unix)]
            fds: serializer.fds.into_iter(),
            #[cfg(windows)]
            handles: serializer.handles.into_iter(),
            #[cfg(windows)]
            sockets: serializer.sockets.into_iter(),
            pos: 0,
        }
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
/// Most objects that store references to files can be serialized automatically, including
/// [`std::fs::File`]. If you need to serialize a custom type with a file descriptor (on Unix) or
/// a handle (on Windows), you can use [`OwnedFd`](std::os::unix::io::OwnedFd) or
/// [`OwnedHandle`](std::os::windows::io::OwnedHandle):
///
/// ```rust
/// use crossmist::{Deserializer, Object, Serializer};
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
/// An implementation of this trait function is safe if the order of serialized types during
/// serialization and deserialization matches, up to serialization layout. See the documentation of
/// [`Deserializer::deserialize`] for more details.
#[allow(private_bounds)]
pub unsafe trait Object: BaseObject {
    /// Serialize a single object into a serializer.
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
