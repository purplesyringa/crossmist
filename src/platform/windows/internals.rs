use crate::{
    Deserializer, Object, Serializer,
    handles::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle},
    subprocess::HANDLE_BROKER,
};
use std::default::Default;
use std::io::Result;
use windows::Win32::{Foundation, System::Threading};

pub(crate) fn serialize_with_handles<T: Object>(value: &T) -> Result<Vec<u8>> {
    let mut s = Serializer::new();
    s.serialize(value);

    let (data, handles) = s.into_parts();
    let mut remote_handles = Vec::new();
    if !handles.is_empty() {
        let broker = HANDLE_BROKER
            .get()
            .expect("broker has not been initialized");

        for handle in handles {
            let mut remote_handle: RawHandle = Default::default();
            unsafe {
                Foundation::DuplicateHandle(
                    Threading::GetCurrentProcess(),
                    handle.as_raw_handle(),
                    broker.process.as_raw_handle(),
                    &mut remote_handle,
                    0,
                    false,
                    Foundation::DUPLICATE_SAME_ACCESS,
                )?;
            }
            remote_handles.push(remote_handle);
        }
    }

    let mut s1 = Serializer::new();
    s1.serialize(&remote_handles);
    s1.write(&data);
    Ok(s1.into_parts().0)
}

pub(crate) unsafe fn deserialize_with_handles<T: Object>(serialized: Vec<u8>) -> Result<T> {
    let mut d = Deserializer::new(serialized, Vec::new());
    let remote_handles: Vec<RawHandle> = unsafe { d.deserialize() };
    let serialized_contents: Vec<u8> = Vec::from(d.get_rest());

    let mut handles = Vec::new();
    if !remote_handles.is_empty() {
        let broker = HANDLE_BROKER
            .get()
            .expect("broker has not been initialized");

        for remote_handle in remote_handles {
            let mut handle: RawHandle = Default::default();
            unsafe {
                Foundation::DuplicateHandle(
                    broker.process.as_raw_handle(),
                    remote_handle,
                    Threading::GetCurrentProcess(),
                    &mut handle,
                    0,
                    false,
                    Foundation::DUPLICATE_CLOSE_SOURCE | Foundation::DUPLICATE_SAME_ACCESS,
                )?;
            }
            let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
            handles.push(handle);
        }
    }

    Ok(unsafe { Deserializer::new(serialized_contents, handles).deserialize() })
}
