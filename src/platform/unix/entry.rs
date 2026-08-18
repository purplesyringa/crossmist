use crate::{Deserializer, Receiver, Serializer, asynchronous::handle_entry};
use rustix::io::{FdFlags, fcntl_setfd};
use std::os::unix::io::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};

pub(crate) fn crossmist_main(mut args: std::env::Args) -> ! {
    let fd = unsafe {
        parse_fd(
            &args
                .next()
                .expect("Expected one CLI argument for crossmist"),
        )
    };

    enable_cloexec(fd.as_fd()).expect("Failed to set O_CLOEXEC for the file descriptor");

    let mut entry_rx = unsafe { Receiver::<(Vec<u8>, Vec<RawFd>)>::from_raw_fd(fd.as_raw_fd()) };

    let (data, fds) = entry_rx
        .recv()
        .expect("Failed to read entry for crossmist")
        .expect("No entry passed");

    std::mem::forget(entry_rx);

    let fds = fds
        .into_iter()
        .map(|fd| unsafe { OwnedFd::from_raw_fd(fd) })
        .collect::<Vec<_>>();

    for fd in &fds {
        enable_cloexec(fd.as_fd()).expect("Failed to set O_CLOEXEC for the file descriptor");
    }

    handle_entry(Deserializer::from(Serializer { data, fds }), fd.as_raw_fd());
}

unsafe fn parse_fd(s: &str) -> OwnedFd {
    unsafe { OwnedFd::from_raw_fd(s.parse().expect("Failed to parse fd")) }
}

pub(crate) fn disable_cloexec(fd: BorrowedFd<'_>) -> std::io::Result<()> {
    fcntl_setfd(fd, FdFlags::empty())?;
    Ok(())
}
pub(crate) fn enable_cloexec(fd: BorrowedFd<'_>) -> std::io::Result<()> {
    fcntl_setfd(fd, FdFlags::CLOEXEC)?;
    Ok(())
}
