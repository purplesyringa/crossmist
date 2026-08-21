use crate::{
    Sender,
    asynchronous::handle_entry,
    subprocess::{get_sequence_number, set_broker},
};
use std::os::windows::io::{
    AsHandle, AsRawHandle, AsRawSocket, BorrowedHandle, FromRawHandle, FromRawSocket,
    IntoRawHandle, OwnedHandle, OwnedSocket, RawSocket,
};
use windows::Win32::{Foundation, Networking::WinSock, System::Threading};

pub(crate) fn crossmist_main(mut args: std::env::Args) -> ! {
    let mut data = WinSock::WSADATA::default();
    if unsafe { WinSock::WSAStartup(0x0202, &raw mut data) } != 0 {
        panic!(
            "Failed to initialize WinSock: {:?}",
            std::io::Error::from_raw_os_error(unsafe { WinSock::WSAGetLastError() }.0)
        );
    }

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
            .expect("failed to open parent")
            .0,
        )
    };

    // Validate that the process we've just opened is the intended parent and the PID wasn't reused
    let seqnum = unsafe { get_sequence_number(parent.as_raw_handle()) }
        .expect("failed to get parent's creation time");
    let expected_seqnum = args
        .next()
        .expect("Expected six CLI arguments for crossmist")
        .parse()
        .expect("Failed to parse sequence number");
    assert!(seqnum == expected_seqnum, "PID reuse detected");

    // If our parent dies while copying the handles, this will safely crash.
    let [broker_process, broker_job, socket] = core::array::from_fn(|_| unsafe {
        parse_handle(
            parent.as_handle(),
            &args
                .next()
                .expect("Expected six CLI arguments for crossmist"),
        )
    });
    let socket = unsafe { OwnedSocket::from_raw_socket(socket.into_raw_handle() as RawSocket) };

    let broker_pid = args
        .next()
        .expect("Expected six CLI arguments for crossmist")
        .parse()
        .expect("Failed to parse broker PID");

    set_broker(broker_process, broker_pid, broker_job);

    // Notify the parent that they can stop keeping the handles alive
    let mut signal = unsafe { Sender::from_raw_socket(socket.as_raw_socket()) };
    signal.send(()).expect("Failed to signal");
    core::mem::forget(signal);

    handle_entry(socket);
}

unsafe fn parse_handle(parent: BorrowedHandle<'_>, s: &str) -> OwnedHandle {
    let remote_handle =
        core::ptr::without_provenance_mut(s.parse().expect("Failed to parse handle"));
    let mut handle = Default::default();
    unsafe {
        Foundation::DuplicateHandle(
            Foundation::HANDLE(parent.as_raw_handle()),
            Foundation::HANDLE(remote_handle),
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
