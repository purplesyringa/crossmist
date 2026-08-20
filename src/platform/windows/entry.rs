use crate::{Sender, asynchronous::handle_entry, subprocess::load_init_handles};
use std::os::windows::io::{AsRawSocket, FromRawSocket};
use windows::Win32::Networking::WinSock;

pub(crate) fn crossmist_main(_args: std::env::Args) -> ! {
    let mut data = WinSock::WSADATA::default();
    if unsafe { WinSock::WSAStartup(0x0202, &raw mut data) } != 0 {
        panic!(
            "Failed to initialize WinSock: {:?}",
            std::io::Error::from_raw_os_error(unsafe { WinSock::WSAGetLastError() }.0)
        );
    }

    let socket = unsafe { load_init_handles() };

    // Notify the parent that they can stop keeping the handles alive
    let mut signal = unsafe { Sender::from_raw_socket(socket.as_raw_socket()) };
    signal.send(()).expect("Failed to signal");
    core::mem::forget(signal);

    // By the time we enter user code, we'll have already received the entry message, at which point
    // we know our parent has already made the job we're in non-kill-on-close, so we can run user
    // code safely.
    handle_entry(socket);
}
