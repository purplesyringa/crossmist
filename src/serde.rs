//! Serialization and deserialization.
//!
//! This is *not* the well-known `serde` crate. We use custom serialization methods because we need
//! to serialize not only data structures, but objects with real-world side-effects, e.g. files.

use crate::handles::{OwnedHandle, RawHandle};
use std::any::Any;
use std::collections::{hash_map, HashMap};
use std::num::NonZeroUsize;
use std::os::raw::c_void;

/// Stateful serialization.
pub struct Serializer {
    data: Vec<u8>,
    handles: Option<Vec<RawHandle>>,
    cyclic_ids: HashMap<*const c_void, NonZeroUsize>,
}

impl Serializer {
    /// Create a new serializer.
    pub fn new() -> Self {
        Serializer {
            data: Vec::new(),
            handles: Option::from(Vec::new()),
            cyclic_ids: HashMap::new(),
        }
    }

    /// Append chunk of serialize data.
    pub fn write(&mut self, data: &[u8]) {
        self.data.extend_from_slice(data);
    }

    /// Append serialized data of an object.
    pub fn serialize<T: Object + ?Sized>(&mut self, data: &T) {
        data.serialize_self(self);
    }

    /// Store a file handle, returning its index.
    pub fn add_handle(&mut self, handle: RawHandle) -> usize {
        let handles = self
            .handles
            .as_mut()
            .expect("add_handle cannot be called after drain_handles");
        handles.push(handle);
        handles.len() - 1
    }

    /// Get a list of added file handles.
    pub fn drain_handles(&mut self) -> Vec<RawHandle> {
        self.handles
            .take()
            .expect("drain_handles can only be called once")
    }

    /// Check if an object has already been serialized in this session and return its index.
    pub fn learn_cyclic(&mut self, ptr: *const c_void) -> Option<NonZeroUsize> {
        let len_before = self.cyclic_ids.len();
        match self.cyclic_ids.entry(ptr) {
            hash_map::Entry::Occupied(occupied) => Some(*occupied.get()),
            hash_map::Entry::Vacant(vacant) => {
                vacant.insert(NonZeroUsize::new(len_before + 1).expect("Too many cyclic objects"));
                None
            }
        }
    }

    /// Extract serialized data.
    pub fn into_vec(self) -> Vec<u8> {
        self.data
    }
}

impl IntoIterator for Serializer {
    type Item = u8;
    type IntoIter = <Vec<u8> as IntoIterator>::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        self.data.into_iter()
    }
}

/// Stateful deserialization.
pub struct Deserializer {
    data: Vec<u8>,
    handles: Vec<Option<OwnedHandle>>,
    pos: usize,
    cyclics: Vec<Box<dyn Any>>,
}

impl Deserializer {
    /// Start deserializing data obtain from a [`Serializer`].
    pub fn from(data: Vec<u8>, handles: Vec<OwnedHandle>) -> Self {
        Deserializer {
            data,
            handles: handles.into_iter().map(Some).collect(),
            pos: 0,
            cyclics: Vec::new(),
        }
    }

    /// Fill the buffer from internal data.
    pub fn read(&mut self, data: &mut [u8]) {
        data.clone_from_slice(&self.data[self.pos..self.pos + data.len()]);
        self.pos += data.len();
    }

    /// Deserialize an object of a given type from self.
    pub fn deserialize<T: Object>(&mut self) -> T {
        T::deserialize_self(self)
    }

    /// Extract a handle by an index.
    pub fn drain_handle(&mut self, idx: usize) -> OwnedHandle {
        self.handles[idx]
            .take()
            .expect("drain_handle can only be called once for a particular index")
    }

    /// Store a reference to a newly build potentially cyclic object.
    pub fn learn_cyclic<T: 'static>(&mut self, obj: T) {
        self.cyclics.push(Box::new(obj));
    }

    /// Get a reference to an earlier built object.
    pub fn get_cyclic<T: 'static>(&self, id: NonZeroUsize) -> &T {
        self.cyclics[id.get() - 1]
            .downcast_ref()
            .expect("The cyclic object is of unexpected type")
    }
}

/// A serializable object.
///
/// This trait can and should be implemented automatically via `#[derive(Object)]`. It is already
/// implemented for most types from the standard library for which it can reasonably be implemented.
///
/// If you do need to implement this manually, use the following template:
///
/// ```rust
/// use crossmist::{Deserializer, Object, Serializer};
///
/// struct SimplePair<T: Object, U: Object> {
///     first: T,
///     second: U,
/// }
///
/// impl<T: Object, U: Object> Object for SimplePair<T, U> {
///     fn serialize_self(&self, s: &mut Serializer) {
///         s.serialize(&self.first);
///         s.serialize(&self.second);
///     }
///     fn deserialize_self(d: &mut Deserializer) -> Self
///     where
///         Self: Sized
///     {
///         let first = d.deserialize::<T>();
///         let second = d.deserialize::<U>();
///         Self { first, second }
///     }
///     fn deserialize_on_heap<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a>
///     where
///         Self: 'a
///     {
///         Box::new(Self::deserialize_self(d))
///     }
/// }
/// ```
///
/// The contents of `serialize_self` and `deserialize_self` should be fairly obvious.
/// `deserialize_on_heap` must *always* contain this exact code (up to equivalent changes): this is
/// a (somewhat unsafe) technical detail.
///
///
/// ## Cyclic structures
///
/// Occasionally, you might need to serialize recursive structures that might contain loops. You're
/// probably better of using [`std::rc::Rc`] or [`std::sync::Arc`] or rewriting your structures, but
/// if nothing better comes to your mind, you can do the same thing that `Rc` does:
///
/// ```rust
/// # use crossmist::{Deserializer, Object, Serializer};
/// # use std::os::raw::c_void;
/// # use std::rc::Rc;
/// struct CustomRc<T: 'static>(Rc<T>);
///
/// impl<T: 'static + Object> Object for CustomRc<T> {
///     fn serialize_self(&self, s: &mut Serializer) {
///         // Any unique identifier works, but it must be *globally* unique, not just for objects
///         // of the same type.
///         match s.learn_cyclic(Rc::as_ptr(&self.0) as *const c_void) {
///             None => {
///                 // This is the first time we see this object -- encode a marker followed by its
///                 // contents. Under the hood, learn_cyclic remembers this object.
///                 s.serialize(&0usize);
///                 s.serialize(&*self.0);
///             }
///             Some(id) => {
///                 // We have seen this object before -- store its ID instead
///                 s.serialize(&id);
///             }
///         }
///     }
///     fn deserialize_self(d: &mut Deserializer) -> Self {
///         let id = d.deserialize::<usize>();
///         match std::num::NonZeroUsize::new(id) {
///             None => {
///                 // If 0 is stored, this is the first time we see this object -- decode its
///                 // contents
///                 let rc = Rc::<T>::new(d.deserialize());
///                 // Tell the deserializer about this object. Note that you don't specify the ID:
///                 // learn_cyclic infers it automatically. To make sure numeration is consistent
///                 // with the serializer, call learn_cyclic in the same order in both. For
///                 // instance, when encoding a set, make sure that data is serialized in the same
///                 // order as it is deserialized. This should already be the case unless you
///                 // serialize data in a very bizarre way. Also, notice that learn_cyclic does not
///                 // have to store the exact object you are deserializing in: in this case, we
///                 // store the Rc itself, not CustomRc.
///                 d.learn_cyclic(rc.clone());
///                 Self(rc)
///             }
///             Some(id) => {
///                 // If a non-zero value is stored, this is an ID of an already existing object.
///                 // Notice that you must specify the type of the object you expect to be stored.
///                 // get_cyclic returns a reference to the object. In case of Rc, cloning it is
///                 // sufficient.
///                 Self(d.get_cyclic::<Rc<T>>(id).clone())
///             }
///         }
///     }
///     fn deserialize_on_heap<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a>
///     where
///         T: 'a,
///     {
///         Box::new(Self::deserialize_self(d))
///     }
/// }
/// ```
///
///
/// ## File descriptors
///
/// Sometimes, you might need to serialize objects that store references to files. This is done
/// automatically for [`std::fs::File`], [`OwnedHandle`] and related types, but if you have a
/// different runtime, things might get a bit complicated.
///
/// In this case, the following example should be of help:
///
/// ```rust
/// # use crossmist::{
/// #     handles::{AsRawHandle, OwnedHandle},
/// #     Deserializer, Object, Serializer,
/// # };
/// # use std::fs::File;
/// struct CustomFile(std::fs::File);
///
/// impl Object for CustomFile {
///     fn serialize_self(&self, s: &mut Serializer) {
///         // add_handle memorizes the handle (fd) and returns its ID
///         let handle = s.add_handle(self.0.as_raw_handle());
///         s.serialize(&handle)
///     }
///     fn deserialize_self(d: &mut Deserializer) -> Self {
///         // Deserializing OwnedHandle results in the ID being resolved into the handle, which can
///         // then be used to create the instance of the object we are deserializing
///         Self(d.deserialize::<OwnedHandle>().into())
///     }
///     fn deserialize_on_heap<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a> {
///         Box::new(Self::deserialize_self(d))
///     }
/// }
/// ```
pub trait Object {
    fn serialize_self(&self, s: &mut Serializer);
    fn deserialize_self(d: &mut Deserializer) -> Self
    where
        Self: Sized;
    fn deserialize_on_heap<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a>
    where
        Self: 'a;
}
