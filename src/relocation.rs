use crate::{Deserializer, Object, Serializer};

// This needs to be a singleton to prevent different codegen units from using different copies of
// the function. See also: https://github.com/alecmocatta/relative/pull/2
static BASE_ADDRESS: fn(()) = std::mem::drop::<()>;

#[derive(Debug)]
#[repr(transparent)]
pub(crate) struct RelocatablePtr<T>(pub(crate) *const T);

// Implement Clone/Copy even for T: !Clone/Copy
impl<T> Clone for RelocatablePtr<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for RelocatablePtr<T> {}

unsafe impl<T> Object for RelocatablePtr<T> {
    fn serialize_self(self, s: &mut Serializer) {
        // Don't bother exposing provenance -- it won't work in another process anyway.
        s.serialize(self.0.addr().wrapping_sub(BASE_ADDRESS as usize));
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        // `RelocatablePtr` is used for `static`s and pointers to functions present at startup. Both
        // are effectively pre-exposed by the as-if rule: they are visible via FFI and there is no
        // proof that they weren't exposed by life-before-main.
        unsafe {
            Self(core::ptr::with_exposed_provenance(
                (BASE_ADDRESS as usize).wrapping_add(d.deserialize()),
            ))
        }
    }
}
