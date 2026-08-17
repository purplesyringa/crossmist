use crate::{
    Deserializer, Object, Serializer,
    owning_ref::{OwningRef, WithOwningRef},
    relocation::RelocatablePtr,
};

// XXX: Rust doesn't guarantee the order of data and vtable pointers, so this can break. This should
// eventually be replaced with the metadata API.
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
        if size_of::<&T>() == size_of::<usize>() {
            Self::Sized
        } else if size_of::<&T>() == size_of::<DynFatPtr>() {
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

unsafe impl<T: Object + ?Sized> Object for Box<T> {
    fn serialize_self(self, s: &mut Serializer) {
        // Object inherits from BaseObject, which only has two implemetors: an explicit blanket impl
        // for Sized types and `dyn Trait` where `Trait: BaseObject`, so these two are the only
        // possible metadatas. Slices are handled in another impl, custom DSTs are unsupported.

        if TypeClass::of::<T>() == TypeClass::Dyn {
            let fat_ptr = unsafe { std::mem::transmute_copy::<&T, DynFatPtr>(&self.as_ref()) };
            s.serialize(RelocatablePtr(fat_ptr.vtable));
        }

        // On nightly, the vtable is sufficient to deserialize the object. On stable, we can't call
        // any methods on `T: ?Sized` without having an instance of `T` (at least until `try_as_dyn`
        // lands), forcing us to pass an explicit deserializer function. Even then, we still have to
        // pass the vtable, since the deserializer function cannot know both the concrete type and
        // the specific subtrait of `Object` to emit the vtable for at the same time.
        #[cfg(not(feature = "nightly"))]
        s.serialize(RelocatablePtr(
            self.as_ref().deserialize_on_heap_get() as *const ()
        ));

        self.with_owning_ref(|r: OwningRef<'_, T>| s.serialize_ref(r));
    }

    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        unsafe {
            let mut pointer: *mut T = match TypeClass::of::<T>() {
                // `std::ptr::null_mut` doesn't work for `T: ?Sized`.
                TypeClass::Sized => std::mem::transmute_copy::<usize, *mut T>(&0usize),
                TypeClass::Dyn => std::mem::transmute_copy::<DynFatPtr, *mut T>(&DynFatPtr {
                    data: std::ptr::null(),
                    vtable: d.deserialize::<RelocatablePtr<()>>().0,
                }),
            };

            #[cfg(feature = "nightly")]
            let data = pointer.deserialize_on_heap_ptr(d);
            #[cfg(not(feature = "nightly"))]
            let data = std::mem::transmute::<
                RelocatablePtr<()>,
                unsafe fn(&mut Deserializer) -> *mut (),
            >(d.deserialize::<RelocatablePtr<()>>())(d);

            // Patch the data part of the pointer without checking whether it's thin or fat.
            (&raw mut pointer).cast::<*mut ()>().write(data);

            Box::from_raw(pointer)
        }
    }
}

unsafe impl<T: Object> Object for Box<[T]> {
    fn serialize_self(self, s: &mut Serializer) {
        s.serialize(self.len());
        self.with_owning_ref(|slice| s.serialize_slice(slice));
    }
    unsafe fn deserialize_self(d: &mut Deserializer) -> Self {
        unsafe { d.deserialize::<Vec<T>>() }.into_boxed_slice()
    }
}
