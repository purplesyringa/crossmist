//! Serialization and deserialization.
//!
//! This is *not* the well-known `serde` crate. We use custom serialization methods because we need
//! to serialize not only data structures, but objects with real-world side-effects, e.g. files.

use crate::{Object, owning_ref::OwningRef};
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
    /// use crossmist::serde::{Deserializer, Serializer};
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
    /// use crossmist::serde::{Deserializer, Serializer};
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
