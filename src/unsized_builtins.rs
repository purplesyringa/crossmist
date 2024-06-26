use crate::{relocation::RelocatablePtr, Deserializer, NonTrivialObject, Object, Serializer};
use std::io::Result;

#[repr(C)]
struct DynFatPtr {
    data: *const (),
    vtable: *const (),
}

#[derive(PartialEq)]
enum TypeClass {
    Sized,
    Dyn,
}
impl TypeClass {
    const fn of<T: ?Sized>() -> Self {
        if std::mem::size_of::<&T>() == std::mem::size_of::<usize>() {
            Self::Sized
        } else if std::mem::size_of::<&T>() == std::mem::size_of::<DynFatPtr>() {
            Self::Dyn
        } else {
            panic!(
                "Unexpected pointer size. You are probably trying to serialize Box<&dyn TraitA + \
                 TraitB>, which crossmist does not support, because this feature was not present \
                 in rustc when this crate was published.",
            );
        }
    }
}

unsafe impl<T: Object + ?Sized> NonTrivialObject for Box<T> {
    fn serialize_self_non_trivial(&self, s: &mut Serializer) {
        // Object is only implemented for types that implement NonTrivialObject, which inherits
        // Sized, and `dyn Trait` where `Trait: Object`. Therefore, the only possible Ts here are
        // sized types and `dyn Trait`. Slices are handled in another impl block, custom DSTs are
        // not supported at all.

        if TypeClass::of::<T>() == TypeClass::Dyn {
            let fat_ptr = unsafe { std::mem::transmute_copy::<&T, DynFatPtr>(&self.as_ref()) };
            s.serialize(&RelocatablePtr(fat_ptr.vtable));
        }

        #[cfg(not(feature = "nightly"))]
        s.serialize(&RelocatablePtr(
            self.as_ref().deserialize_on_heap_get() as *const ()
        ));

        self.as_ref().serialize_self(s);
    }

    unsafe fn deserialize_self_non_trivial(d: &mut Deserializer) -> Result<Self> {
        let mut pointer: *mut T = match TypeClass::of::<T>() {
            TypeClass::Sized => std::mem::transmute_copy::<usize, *mut T>(&0usize),
            TypeClass::Dyn => std::mem::transmute_copy::<DynFatPtr, *mut T>(&DynFatPtr {
                data: std::ptr::null(),
                vtable: d.deserialize::<RelocatablePtr<()>>()?.0,
            }),
        };

        #[cfg(feature = "nightly")]
        let pointer_thin_part = pointer.deserialize_on_heap_ptr(d)?;
        #[cfg(not(feature = "nightly"))]
        let pointer_thin_part = std::mem::transmute::<
            RelocatablePtr<()>,
            unsafe fn(&mut Deserializer) -> Result<*mut ()>,
        >(d.deserialize::<RelocatablePtr<()>>()?)(d)?;

        (&mut pointer as *mut *mut T as *mut *mut ()).write(pointer_thin_part);

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
