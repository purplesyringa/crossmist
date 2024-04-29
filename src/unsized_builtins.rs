use crate::{
    imp::implements, relocation::RelocatablePtr, Deserializer, NonTrivialObject, Object, Serializer,
};
use std::io::Result;

#[repr(C)]
struct DynFatPtr {
    data: *const (),
    vtable: *const (),
}

unsafe impl<T: Object + ?Sized> NonTrivialObject for Box<T> {
    fn serialize_self_non_trivial(&self, s: &mut Serializer) {
        #[cfg(not(feature = "nightly"))]
        s.serialize(&RelocatablePtr(self.get_heap_deserializer() as *const ()));

        if implements!(T: Sized) {
            self.as_ref().serialize_self(s);
        } else {
            // Object is only implemented for types that implement NonTrivialObject, which inherits
            // Sized, and `dyn Trait` where `Trait: Object`. Therefore, the only possible T here is
            // `dyn Trait`. Slices are handled in another impl block, custom DSTs are not supported
            // at all.
            assert!(
                std::mem::size_of::<&T>() == std::mem::size_of::<DynFatPtr>(),
                "Unexpected fat pointer size. You are probably trying to serialize Box<&dyn \
                 TraitA + TraitB>, which crossmist does not support, because this feature was not \
                 present in rustc when this crate was published.",
            );
            let fat_ptr = unsafe { std::mem::transmute_copy::<&T, DynFatPtr>(&self.as_ref()) };
            s.serialize(&RelocatablePtr(fat_ptr.vtable));
            self.as_ref().serialize_self(s);
        }
    }

    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Result<Self> {
        #[cfg(not(feature = "nightly"))]
        let heap_deserializer = std::mem::transmute::<
            RelocatablePtr<()>,
            unsafe fn(&mut Deserializer) -> Result<*mut ()>,
        >(d.deserialize::<RelocatablePtr<()>>()?);

        let mut pointer: *mut T = if implements!(T: Sized) {
            assert!(std::mem::size_of::<&T>() == std::mem::size_of::<usize>());
            std::mem::transmute_copy::<usize, *mut T>(&0usize)
        } else {
            assert!(
                std::mem::size_of::<&T>() == std::mem::size_of::<DynFatPtr>(),
                "Unexpected fat pointer size. You are probably trying to serialize Box<&dyn \
                 TraitA + TraitB>, which crossmist does not support, because this feature was not \
                 present in rustc when this crate was published.",
            );
            std::mem::transmute_copy::<DynFatPtr, *mut T>(&DynFatPtr {
                data: std::ptr::null(),
                vtable: d.deserialize::<RelocatablePtr<()>>()?.0,
            })
        };

        #[cfg(feature = "nightly")]
        std::ptr::copy_nonoverlapping(
            &Box::into_raw(pointer.deserialize_on_heap(d)?) as *const *mut dyn Object
                as *const *mut (),
            &mut pointer as *mut *mut T as *mut *mut (),
            1,
        );
        #[cfg(not(feature = "nightly"))]
        (&mut pointer as *mut *mut T as *mut *mut ()).write(heap_deserializer(d)?);

        Ok(Box::from_raw(pointer))
    }
}

unsafe impl<T: Object> NonTrivialObject for Box<[T]> {
    fn serialize_self_non_trivial(&self, s: &mut Serializer) {
        s.serialize(&self.len());
        s.serialize_slice(self.as_ref());
    }
    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Result<Self> {
        Ok(d.deserialize::<Vec<T>>()?.into_boxed_slice())
    }
}
