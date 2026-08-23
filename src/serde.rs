//! Serialization and deserialization.
//!
//! This is not [the `serde` crate](https://serde.rs). crossmist uses a custom serialization
//! protocol to enable safely serializing file descriptors, sockets, and handles. This protocol is
//! not covered by semver stability guarantees and shouldn't be used for any purpose other than
//! sending data over crossmist channels. These types are only public to enable manual [`Object`]
//! implementations, so most methods are private.
//!
//! # Safety and security
//!
//! Deserialization is trusted: deserializing malformed data can lead to UB, and deserializing
//! untrusted data can lead to security vulnerabilities. crossmist is not a security boundary.
//!
//! # Asynchronous data
//!
//! Serializing asynchronous objects requires some extra care.
//!
//! Since Rust allows [future cancellation](https://google.github.io/comprehensive-rust/concurrency/async-pitfalls/cancellation.html),
//! the compiler permits an async object like [`tokio::fs::File`] to be sent over a channel before
//! all outstanding operations on it complete. When this happens, serialization panics.
//!
//! During deserialization, `tokio` async objects often try to register themselves in the reactor.
//! If the `tokio` runtime is not running when such an object is received, deserialization panics.

use crate::{Object, owning_ref::OwningRef};
use std::fmt;
#[cfg(unix)]
use std::os::unix::io::OwnedFd;
#[cfg(windows)]
use std::os::windows::io::{OwnedHandle, OwnedSocket};

// Functions in this module don't write/read the serializer/deserializer fields directly -- instead
// the fields are made crate-public and Object implementations for builtins access them. This
// reduces code duplication and simplifies the API surface.

/// Serializer.
///
/// Accumulates binary data and OS resources corresponding to the serialized object.
///
/// See [module-level documentation](self) for more details.
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
    // Has to public for use in tests.
    #[doc(hidden)]
    /// Create a new serializer.
    pub fn private_new() -> Self {
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

    /// Append an object to the stream.
    ///
    /// The object is consumed so that unique resources owned by the object (e.g. file descriptors)
    /// are not duplicated.
    pub fn serialize<T: Object>(&mut self, data: T) {
        data.serialize_self(self);
    }

    /// Append an object taken by an owning reference to support unsized types.
    pub(crate) fn serialize_ref<T: Object + ?Sized>(&mut self, data: OwningRef<'_, T>) {
        unsafe { data.leak().serialize_taking(self) };
    }

    /// Append a slice of objects, as if calling [`Serializer::serialize`] for each element, but
    /// more optimized.
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

/// Deserializer.
///
/// Stores consumable binary data and OS resources.
///
/// See [module-level documentation](self) for more details.
pub struct Deserializer {
    pub(crate) data: Vec<u8>,
    pub(crate) pos: usize,
    #[cfg(unix)]
    pub(crate) fds: std::vec::IntoIter<OwnedFd>,
    #[cfg(windows)]
    pub(crate) handles: std::vec::IntoIter<OwnedHandle>,
    #[cfg(windows)]
    pub(crate) sockets: std::vec::IntoIter<OwnedSocket>,
}

impl Deserializer {
    // `Serializer` is a good enough vocabulary type to build `Deserializer` from, and this
    // conversion has reasonable semantics.
    #[doc(hidden)]
    pub fn private_new(serializer: Serializer) -> Self {
        Self {
            data: serializer.data,
            pos: 0,
            #[cfg(unix)]
            fds: serializer.fds.into_iter(),
            #[cfg(windows)]
            handles: serializer.handles.into_iter(),
            #[cfg(windows)]
            sockets: serializer.sockets.into_iter(),
        }
    }

    /// Load an object of a given type from the stream.
    ///
    /// # Safety
    ///
    /// This function is only safe to call if the order of serialized types during serialization and
    /// deserialization matches. It doesn't perform any sanity checks, and holding it wrong may lead
    /// to UB.
    ///
    /// Correct:
    ///
    /// ```
    /// # use crossmist::serde::{Deserializer, Serializer};
    /// # let mut serializer = Serializer::private_new();
    /// serializer.serialize(1u8);
    /// serializer.serialize(2u16);
    /// # let mut deserializer = Deserializer::private_new(serializer);
    /// // ...
    /// unsafe {
    ///     assert_eq!(deserializer.deserialize::<u8>(), 1);
    ///     assert_eq!(deserializer.deserialize::<u16>(), 2);
    /// }
    /// ```
    ///
    /// Incorrect:
    ///
    /// ```no_run
    /// # use crossmist::serde::{Deserializer, Serializer};
    /// # let mut serializer = Serializer::private_new();
    /// serializer.serialize(1u8);
    /// serializer.serialize(2u16);
    /// # let mut deserializer = Deserializer::private_new(serializer);
    /// // ...
    /// unsafe {
    ///     deserializer.deserialize::<u16>();
    ///     deserializer.deserialize::<u8>();
    /// }
    /// ```
    pub unsafe fn deserialize<T: Object>(&mut self) -> T {
        unsafe { T::deserialize_self(self) }
    }
}

impl fmt::Debug for Deserializer {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        // Omit internal state because it's not user-friendly.
        fmt.debug_struct("Deserializer").finish()
    }
}
