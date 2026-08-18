use crate::{
    Deserializer, Object, Receiver, Sender, Serializer,
    asynchronous::handle_entry,
    handles::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, IntoRawHandle, OwnedHandle},
    subprocess::{get_creation_time, set_broker},
};
use windows::Win32::{Foundation, System::Threading};

// XXX: very hacky
struct FakeDeserializer(Deserializer);
unsafe impl Object for FakeDeserializer {
    fn serialize_self(self, _serializer: &mut Serializer) {
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
    let ppid = args
        .next()
        .expect("Expected six CLI arguments for crossmist")
        .parse()
        .expect("Failed to parse PPID");
    let parent = unsafe {
        OwnedHandle::from_raw_handle(
            Threading::OpenProcess(
                Threading::PROCESS_DUP_HANDLE | Threading::PROCESS_QUERY_LIMITED_INFORMATION,
                false,
                ppid,
            )
            .expect("failed to open parent"),
        )
    };

    // Validate that the process we've just opened is the intended parent and the PID wasn't reused
    let creation_time = unsafe { get_creation_time(parent.as_raw_handle()) }
        .expect("failed to get parent's creation time");
    let expected_creation_time = args
        .next()
        .expect("Expected six CLI arguments for crossmist")
        .parse()
        .expect("Failed to parse creation time");
    assert!(
        creation_time == expected_creation_time,
        "PID reuse detected"
    );

    // If our parent dies while copying the handles, this will safely crash.
    let [broker_process, broker_job, handle_tx, handle_rx] = core::array::from_fn(|_| unsafe {
        parse_handle(
            parent.as_handle(),
            &args
                .next()
                .expect("Expected six CLI arguments for crossmist"),
        )
    });

    // Notify the parent that they can stop keeping the handles alive
    let mut output_tx = unsafe { Sender::from_raw_handle(handle_tx.into_raw_handle()) };
    output_tx.send(()).expect("Failed to signal");
    let handle_tx = unsafe { OwnedHandle::from_raw_handle(output_tx.into_raw_handle()) };

    set_broker(broker_process, broker_job);

    let deserializer =
        unsafe { Receiver::<FakeDeserializer>::from_raw_handle(handle_rx.into_raw_handle()) }
            .recv()
            .expect("failed to read entry for crossmist")
            .expect("no entry passed")
            .0;
    handle_entry(deserializer, handle_tx.as_raw_handle());
}

unsafe fn parse_handle(parent: BorrowedHandle<'_>, s: &str) -> OwnedHandle {
    let remote_handle = Foundation::HANDLE(core::ptr::without_provenance_mut(
        s.parse().expect("Failed to parse handle"),
    ));
    let mut handle = Default::default();
    unsafe {
        Foundation::DuplicateHandle(
            parent.as_raw_handle(),
            remote_handle,
            Threading::GetCurrentProcess(),
            &mut handle,
            0,
            false,
            Foundation::DUPLICATE_SAME_ACCESS,
        )
    }
    .expect("failed to steal handle");
    unsafe { OwnedHandle::from_raw_handle(handle) }
}
