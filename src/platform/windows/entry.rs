use crate::{
    Deserializer, Object, Receiver, Serializer,
    asynchronous::handle_entry,
    handles::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, IntoRawHandle, OwnedHandle},
    subprocess::set_broker,
};
use windows::Win32::Foundation;

// XXX: very hacky
struct FakeDeserializer(Deserializer);
unsafe impl Object for FakeDeserializer {
    fn serialize_self<'a>(&'a self, _serializer: &mut Serializer<'a>) {
        unreachable!()
    }
    unsafe fn deserialize_self(deserializer: &mut Deserializer) -> Self {
        Self(core::mem::replace(
            deserializer,
            Deserializer::new(Vec::new(), Vec::new()),
        ))
    }
}

pub(crate) fn crossmist_main(mut args: std::env::Args) -> ! {
    let [broker_process, broker_job, handle_tx, handle_rx] = core::array::from_fn(|_| unsafe {
        parse_handle(
            &args
                .next()
                .expect("Expected four CLI arguments for crossmist"),
        )
    });

    set_broker(broker_process, broker_job);

    enable_cloexec(handle_tx.as_handle()).expect("Failed to set O_CLOEXEC for the file descriptor");
    enable_cloexec(handle_rx.as_handle()).expect("Failed to set O_CLOEXEC for the file descriptor");

    let deserializer =
        unsafe { Receiver::<FakeDeserializer>::from_raw_handle(handle_rx.into_raw_handle()) }
            .recv()
            .expect("failed to read entry for crossmist")
            .expect("no entry passed")
            .0;
    handle_entry(deserializer, handle_tx.as_raw_handle());
}

unsafe fn parse_handle(s: &str) -> OwnedHandle {
    let handle = Foundation::HANDLE(core::ptr::without_provenance_mut(
        s.parse().expect("Failed to parse handle"),
    ));
    unsafe { OwnedHandle::from_raw_handle(handle) }
}

pub(crate) fn disable_cloexec(handle: BorrowedHandle<'_>) -> std::io::Result<()> {
    unsafe {
        Foundation::SetHandleInformation(
            handle.as_raw_handle(),
            Foundation::HANDLE_FLAG_INHERIT.0,
            Foundation::HANDLE_FLAG_INHERIT,
        )
    }?;
    Ok(())
}
pub(crate) fn enable_cloexec(handle: BorrowedHandle<'_>) -> std::io::Result<()> {
    unsafe {
        Foundation::SetHandleInformation(
            handle.as_raw_handle(),
            Foundation::HANDLE_FLAG_INHERIT.0,
            Foundation::HANDLE_FLAGS::default(),
        )
    }?;
    Ok(())
}
