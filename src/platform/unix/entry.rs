use crate::asynchronous::handle_entry;
use rustix::io::{FdFlags, fcntl_setfd};
use std::os::unix::io::{FromRawFd, OwnedFd};

pub(crate) fn crossmist_main(mut args: std::env::Args) -> ! {
    let fd = unsafe {
        OwnedFd::from_raw_fd(
            args.next()
                .expect("Expected one CLI argument for crossmist")
                .parse()
                .expect("Failed to parse fd"),
        )
    };
    fcntl_setfd(&fd, FdFlags::CLOEXEC).expect("Failed to set O_CLOEXEC for the file descriptor");
    handle_entry(fd);
}
