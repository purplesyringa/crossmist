use crate::{
    handles::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle, RawHandle},
    Deserializer, FnOnceObject, Receiver,
};
use rustix::io::{fcntl_setfd, FdFlags};

pub(crate) fn start_root() {}

pub(crate) fn crossmist_main(mut args: std::env::Args) -> ! {
    let handle = unsafe {
        parse_handle(
            &args
                .next()
                .expect("Expected one CLI argument for crossmist"),
        )
    };

    enable_cloexec(handle.as_handle()).expect("Failed to set O_CLOEXEC for the file descriptor");

    let mut entry_rx =
        unsafe { Receiver::<(Vec<u8>, Vec<RawHandle>)>::from_raw_handle(handle.as_raw_handle()) };

    let (entry_data, entry_handles) = entry_rx
        .recv()
        .expect("Failed to read entry for crossmist")
        .expect("No entry passed");

    std::mem::forget(entry_rx);

    let entry_handles = entry_handles
        .into_iter()
        .map(|handle| unsafe { OwnedHandle::from_raw_handle(handle) })
        .collect::<Vec<_>>();

    for handle in &entry_handles {
        enable_cloexec(handle.as_handle())
            .expect("Failed to set O_CLOEXEC for the file descriptor");
    }

    let mut deserializer = Deserializer::new(entry_data, entry_handles);
    let entry: Box<dyn FnOnceObject<(RawHandle,), Output = i32>> =
        unsafe { deserializer.deserialize() }.expect("Failed to deserialize entry");
    std::process::exit(entry.call_object_once((handle.as_raw_handle(),)))
}

unsafe fn parse_handle(s: &str) -> OwnedHandle {
    OwnedHandle::from_raw_handle(s.parse().expect("Failed to parse fd"))
}

pub(crate) fn disable_cloexec(fd: BorrowedHandle<'_>) -> std::io::Result<()> {
    fcntl_setfd(fd, FdFlags::empty())?;
    Ok(())
}
pub(crate) fn enable_cloexec(fd: BorrowedHandle<'_>) -> std::io::Result<()> {
    fcntl_setfd(fd, FdFlags::CLOEXEC)?;
    Ok(())
}
