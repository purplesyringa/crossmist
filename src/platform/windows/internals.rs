use crate::{
    entry,
    handles::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle},
    Deserializer, Object, Serializer,
};
use std::default::Default;
use std::io::Result;
use windows::Win32::{Foundation, System::Threading};

pub(crate) fn serialize_with_handles<T: Object>(value: &T) -> Result<Vec<u8>> {
    let mut s = Serializer::new();
    s.serialize(value);

    let handles = s.drain_handles();
    let mut dup_handles = Vec::new();
    if !handles.is_empty() {
        let handle_broker = entry::HANDLE_BROKER
            .get()
            .expect("HANDLE_BROKER has not been initialized yet");

        for handle in handles {
            let mut dup_handle: RawHandle = Default::default();
            unsafe {
                Foundation::DuplicateHandle(
                    Threading::GetCurrentProcess(),
                    handle.as_raw_handle(),
                    handle_broker.process.as_raw_handle(),
                    &mut dup_handle,
                    0,
                    false,
                    Foundation::DUPLICATE_SAME_ACCESS,
                )
                .ok()?;
            }
            dup_handles.push(dup_handle);
        }
    }

    let mut s1 = Serializer::new();
    s1.serialize(&dup_handles);
    s1.write(&s.into_vec());
    Ok(s1.into_vec())
}

pub(crate) unsafe fn deserialize_with_handles<T: Object>(serialized: Vec<u8>) -> Result<T> {
    let mut d = Deserializer::new(serialized, Vec::new());
    let handles: Vec<RawHandle> = d.deserialize()?;
    let serialized_contents: Vec<u8> = Vec::from(d.get_rest());

    let mut dup_handles = Vec::new();
    if !handles.is_empty() {
        let handle_broker = entry::HANDLE_BROKER
            .get()
            .expect("HANDLE_BROKER has not been initialized yet");

        for handle in handles {
            let mut dup_handle: RawHandle = Default::default();
            unsafe {
                Foundation::DuplicateHandle(
                    handle_broker.process.as_raw_handle(),
                    handle,
                    Threading::GetCurrentProcess(),
                    &mut dup_handle,
                    0,
                    false,
                    Foundation::DUPLICATE_CLOSE_SOURCE | Foundation::DUPLICATE_SAME_ACCESS,
                )
                .ok()?;
            }
            let dup_handle = unsafe { OwnedHandle::from_raw_handle(dup_handle) };
            dup_handles.push(dup_handle);
        }
    }

    Deserializer::new(serialized_contents, dup_handles).deserialize()
}
