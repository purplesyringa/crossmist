use crate::{Deserializer, Object, Receiver, Serializer, asynchronous::handle_entry};
use rustix::io::{FdFlags, fcntl_setfd};
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd};

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
    let fd = unsafe {
        OwnedFd::from_raw_fd(
            args.next()
                .expect("Expected one CLI argument for crossmist")
                .parse()
                .expect("Failed to parse fd"),
        )
    };

    fcntl_setfd(&fd, FdFlags::CLOEXEC).expect("Failed to set O_CLOEXEC for the file descriptor");

    let mut entry_rx = unsafe { Receiver::<FakeDeserializer>::from_raw_fd(fd.as_raw_fd()) };

    let deserializer = entry_rx
        .recv()
        .expect("failed to read entry for crossmist")
        .expect("no entry passed")
        .0;
    core::mem::forget(entry_rx);

    handle_entry(deserializer, fd.as_raw_fd());
}
