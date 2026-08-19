use crate::{
    Deserializer, Object, Receiver, Sender, Serializer, asynchronous::handle_entry,
    subprocess::load_init_handles,
};
use std::os::windows::io::{AsRawSocket, FromRawSocket};

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

pub(crate) fn crossmist_main(_args: std::env::Args) -> ! {
    let socket = unsafe { load_init_handles() };

    // Notify the parent that they can stop keeping the handles alive
    let mut output_tx = unsafe { Sender::from_raw_socket(socket.as_raw_socket()) };
    output_tx.send(()).expect("Failed to signal");
    core::mem::forget(output_tx);

    let mut entry_rx =
        unsafe { Receiver::<FakeDeserializer>::from_raw_socket(socket.as_raw_socket()) };

    // By the time we receive the first byte, we know that our parent has moved on from spawning the
    // child to sending data, so the job we're in is no longer kill-on-close and we can do user work
    // safely.
    let deserializer = entry_rx
        .recv()
        .expect("failed to read entry for crossmist")
        .expect("no entry passed")
        .0;
    core::mem::forget(entry_rx);

    handle_entry(deserializer, socket.as_raw_socket());
}
