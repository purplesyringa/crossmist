//! A wrapper for objects that require global state to be configured before deserialization.
//!
//! Typically, arguments passed to a cross-process entry function are deserialized shortly after the
//! child process is started and just before the function is executed. This works for most types,
//! but some objects, e.g. [`tokio::fs::File`], cannot be created before a specific action is
//! performed (in this example, before the tokio runtime is started). This crate attempts to handle
//! tokio gracefully, however there might be more cases when intervention is required.
//!
//! In these cases, the following pattern may be used:
//!
//! ```rust
//! use crossmist::{Delayed, func, main, Object};
//!
//! #[derive(Object)]
//! struct ComplexType;
//!
//! #[main]
//! fn main() {
//!     go.run(Delayed::new(ComplexType)).unwrap();
//! }
//!
//! #[func]
//! fn go(argument: Delayed<ComplexType>) {
//!     // TODO: Initialize runtime here
//!     let argument = argument.deserialize();
//!     // Keep going...
//! }
//! ```

use crate::{handles::OwnedHandle, Deserializer, NonTrivialObject, Object, Serializer};

/// A wrapper for objects that require global state to be configured before deserialization.
pub struct Delayed<T: Object> {
    inner: DelayedInner<T>,
}

// Use a private enum to stop the user from matching/creating it manually
enum DelayedInner<T: Object> {
    Serialized(Vec<u8>, Vec<OwnedHandle>),
    Deserialized(T),
}

impl<T: Object> Delayed<T> {
    /// Wrap an object. Use this in the parent process.
    pub fn new(value: T) -> Self {
        Self {
            inner: DelayedInner::Deserialized(value),
        }
    }

    /// Unwrap an object. Use this in the child process after initialization.
    pub fn deserialize(self) -> T {
        match self.inner {
            DelayedInner::Serialized(data, handles) => unsafe {
                Deserializer::new(data, handles).deserialize()
            },
            DelayedInner::Deserialized(_) => {
                panic!("Cannot deserialize a deserialized Delayed value")
            }
        }
    }
}

unsafe impl<T: Object> NonTrivialObject for Delayed<T> {
    fn serialize_self_non_trivial(&self, s: &mut Serializer) {
        match self.inner {
            DelayedInner::Serialized(_, _) => panic!("Cannot serialize a serialized Delayed value"),
            DelayedInner::Deserialized(ref value) => {
                let mut s1 = Serializer::new();
                s1.serialize(value);
                let handles = s1
                    .drain_handles()
                    .into_iter()
                    .map(|handle| s.add_handle(handle))
                    .collect::<Vec<usize>>();
                s.serialize(&handles);
                s.serialize(&s1.into_vec());
            }
        }
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Self {
        let handles = d
            .deserialize::<Vec<usize>>()
            .into_iter()
            .map(|handle| d.drain_handle(handle))
            .collect();
        Delayed {
            inner: DelayedInner::Serialized(d.deserialize(), handles),
        }
    }
}
