use crate::{Deserializer, NonTrivialObject, Serializer};

// This needs to be a singleton to prevent different codegen units from using different copies of
// the function. See also: https://github.com/alecmocatta/relative/pull/2

static BASE_ADDRESS: fn(()) = std::mem::drop::<()>;

#[repr(transparent)]
pub(crate) struct RelocatablePtr<T>(pub(crate) *const T);

impl<T> NonTrivialObject for RelocatablePtr<T> {
    fn serialize_self_non_trivial(&self, s: &mut Serializer) {
        s.serialize(&(self.0 as usize).wrapping_sub(BASE_ADDRESS as usize));
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Self {
        Self((BASE_ADDRESS as usize).wrapping_add(d.deserialize()) as *const T)
    }
}
