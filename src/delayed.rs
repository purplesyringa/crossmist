use crate::{Deserializer, Object, Serializer, handles::OwnedHandle};
use std::fmt;
use std::io::Result;

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
    pub fn deserialize(self) -> Result<T> {
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

impl<T: Object> fmt::Debug for Delayed<T> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("Delayed").finish()
    }
}

unsafe impl<T: Object> Object for Delayed<T> {
    fn serialize_self<'a>(&'a self, s: &mut Serializer<'a>) {
        match self.inner {
            DelayedInner::Serialized(_, _) => panic!("Cannot serialize a serialized Delayed value"),
            DelayedInner::Deserialized(ref value) => {
                let mut s1 = Serializer::new();
                s1.serialize(value);
                let handles = s1.drain_handles();
                s.serialize_temporary(handles.len());
                for handle in handles {
                    s.serialize_handle(handle);
                }
                s.serialize_temporary(s1.into_vec());
            }
        }
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Result<Self> {
        unsafe {
            let handles_len = d.deserialize()?;
            let mut handles = Vec::with_capacity(handles_len);
            for _ in 0..handles_len {
                handles.push(d.deserialize::<OwnedHandle>()?);
            }
            Ok(Delayed {
                inner: DelayedInner::Serialized(d.deserialize()?, handles),
            })
        }
    }
}
