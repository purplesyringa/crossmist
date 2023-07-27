use crate::{
    handles::{FromRawHandle, OwnedHandle, RawHandle},
    Deserializer, FnOnceObject, Receiver,
};

pub(crate) fn start_root() {}

pub(crate) fn crossmist_main(mut args: std::env::Args) -> ! {
    let handle: RawHandle = parse_raw_handle(
        &args
            .next()
            .expect("Expected one CLI argument for crossmist"),
    );

    enable_cloexec(handle).expect("Failed to set O_CLOEXEC for the file descriptor");

    let mut entry_rx = unsafe { Receiver::<(Vec<u8>, Vec<RawHandle>)>::from_raw_handle(handle) };

    let (entry_data, entry_handles) = entry_rx
        .recv()
        .expect("Failed to read entry for crossmist")
        .expect("No entry passed");

    std::mem::forget(entry_rx);

    for handle in &entry_handles {
        enable_cloexec(*handle).expect("Failed to set O_CLOEXEC for the file descriptor");
    }

    let entry_handles = entry_handles
        .into_iter()
        .map(|handle| unsafe { OwnedHandle::from_raw_handle(handle) })
        .collect();

    let entry: Box<dyn FnOnceObject<(RawHandle,), Output = i32>> =
        Deserializer::new(entry_data, entry_handles).deserialize();
    std::process::exit(entry(handle))
}

fn parse_raw_handle(s: &str) -> RawHandle {
    s.parse().expect("Failed to parse fd")
}

pub(crate) fn disable_cloexec(fd: RawHandle) -> std::io::Result<()> {
    nix::fcntl::fcntl(
        fd,
        nix::fcntl::FcntlArg::F_SETFD(nix::fcntl::FdFlag::empty()),
    )?;
    Ok(())
}
pub(crate) fn enable_cloexec(fd: RawHandle) -> std::io::Result<()> {
    nix::fcntl::fcntl(
        fd,
        nix::fcntl::FcntlArg::F_SETFD(nix::fcntl::FdFlag::FD_CLOEXEC),
    )?;
    Ok(())
}
pub(crate) fn disable_nonblock(fd: RawHandle) -> std::io::Result<()> {
    nix::fcntl::fcntl(
        fd,
        nix::fcntl::FcntlArg::F_SETFL(nix::fcntl::OFlag::empty()),
    )?;
    Ok(())
}
pub(crate) fn enable_nonblock(fd: RawHandle) -> std::io::Result<()> {
    nix::fcntl::fcntl(
        fd,
        nix::fcntl::FcntlArg::F_SETFL(nix::fcntl::OFlag::O_NONBLOCK),
    )?;
    Ok(())
}
