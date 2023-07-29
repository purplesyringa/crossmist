use crate::{Deserializer, Object, Serializer};

pub trait PlainOldData: Object {}

pub trait EfficientObject: Object {
    fn serialize_self_efficiently(&self, s: &mut Serializer);
    fn serialize_slice_efficiently(elements: &[Self], s: &mut Serializer)
    where
        Self: Sized;
    fn deserialize_self_efficiently(d: &mut Deserializer) -> Self
    where
        Self: Sized;
    fn deserialize_on_heap_efficiently<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a>
    where
        Self: 'a;
}

impl<T: Object + ?Sized> EfficientObject for T {
    default fn serialize_self_efficiently(&self, s: &mut Serializer) {
        self.serialize_self(s);
    }
    default fn serialize_slice_efficiently(elements: &[Self], s: &mut Serializer)
    where
        Self: Sized,
    {
        for element in elements {
            element.serialize_self(s)
        }
    }
    default fn deserialize_self_efficiently(d: &mut Deserializer) -> Self
    where
        Self: Sized,
    {
        T::deserialize_self(d)
    }
    default fn deserialize_on_heap_efficiently<'a>(
        &self,
        d: &mut Deserializer,
    ) -> Box<dyn Object + 'a>
    where
        Self: 'a,
    {
        self.deserialize_on_heap(d)
    }
}

impl<T: PlainOldData> EfficientObject for T {
    fn serialize_self_efficiently(&self, s: &mut Serializer) {
        s.write(unsafe {
            std::slice::from_raw_parts(self as *const T as *const u8, std::mem::size_of::<T>())
        });
    }
    fn serialize_slice_efficiently(elements: &[T], s: &mut Serializer) {
        s.write(unsafe {
            std::slice::from_raw_parts(
                elements.as_ptr() as *const u8,
                std::mem::size_of::<T>() * elements.len(),
            )
        });
    }
    fn deserialize_self_efficiently(d: &mut Deserializer) -> Self {
        unsafe {
            let mut val = std::mem::MaybeUninit::<T>::uninit();
            d.read(std::slice::from_raw_parts_mut(
                val.as_mut_ptr() as *mut u8,
                std::mem::size_of::<T>(),
            ));
            val.assume_init()
        }
    }
    fn deserialize_on_heap_efficiently<'a>(&self, d: &mut Deserializer) -> Box<dyn Object + 'a>
    where
        Self: 'a,
    {
        Box::new(Self::deserialize_self_efficiently(d))
    }
}
