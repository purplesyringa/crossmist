use crate::{
    Deserializer, Object, Receiver, Sender, Serializer,
    asynchronous::handle_entry,
    subprocess::{get_creation_time, set_broker},
};
use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
use windows::Win32::{
    Foundation::{self, HANDLE},
    System::Threading,
};

// XXX: very hacky
struct FakeDeserializer(Deserializer);
unsafe impl Object for FakeDeserializer {
    fn serialize_self(self, _serializer: &mut Serializer) {
        unreachable!()
    }
    unsafe fn deserialize_self(deserializer: &mut Deserializer) -> Self {
        Self(core::mem::replace(
            deserializer,
            Deserializer::from(Serializer::new()),
        ))
    }
}

pub(crate) fn crossmist_main(mut args: std::env::Args) -> ! {
    let ppid = args
        .next()
        .expect("Expected five CLI arguments for crossmist")
        .parse()
        .expect("Failed to parse PPID");
    let parent = unsafe {
        OwnedHandle::from_raw_handle(
            Threading::OpenProcess(
                Threading::PROCESS_DUP_HANDLE | Threading::PROCESS_QUERY_LIMITED_INFORMATION,
                false,
                ppid,
            )
            .expect("failed to open parent")
            .0,
        )
    };

    // Validate that the process we've just opened is the intended parent and the PID wasn't reused
    let creation_time = unsafe { get_creation_time(parent.as_raw_handle()) }
        .expect("failed to get parent's creation time");
    let expected_creation_time = args
        .next()
        .expect("Expected five CLI arguments for crossmist")
        .parse()
        .expect("Failed to parse creation time");
    assert!(
        creation_time == expected_creation_time,
        "PID reuse detected"
    );

    // If our parent dies while copying the handles, this will safely crash.
    let [broker_process, broker_job, handle] = core::array::from_fn(|_| unsafe {
        parse_handle(
            parent.as_handle(),
            &args
                .next()
                .expect("Expected five CLI arguments for crossmist"),
        )
    });

    // Notify the parent that they can stop keeping the handles alive
    let mut output_tx = unsafe { Sender::from_raw_handle(handle.as_raw_handle()) };
    output_tx.send(()).expect("Failed to signal");
    core::mem::forget(output_tx);

    set_broker(broker_process, broker_job);

    let mut entry_rx =
        unsafe { Receiver::<FakeDeserializer>::from_raw_handle(handle.as_raw_handle()) };

    let deserializer = entry_rx
        .recv()
        .expect("failed to read entry for crossmist")
        .expect("no entry passed")
        .0;
    core::mem::forget(entry_rx);

    handle_entry(deserializer, handle.as_raw_handle());
}

unsafe fn parse_handle(parent: BorrowedHandle<'_>, s: &str) -> OwnedHandle {
    let remote_handle =
        core::ptr::without_provenance_mut(s.parse().expect("Failed to parse handle"));
    let mut handle = Default::default();
    unsafe {
        Foundation::DuplicateHandle(
            HANDLE(parent.as_raw_handle()),
            HANDLE(remote_handle),
            Threading::GetCurrentProcess(),
            &mut handle,
            0,
            false,
            Foundation::DUPLICATE_SAME_ACCESS,
        )
    }
    .expect("failed to steal handle");
    unsafe { OwnedHandle::from_raw_handle(handle.0) }
}
