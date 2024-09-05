use crate::{
    channel, func,
    handles::{FromRawHandle, OwnedHandle, RawHandle},
    Deserializer, FnOnceObject, Receiver, Sender,
};
use std::default::Default;
use std::mem::ManuallyDrop;
use std::sync::OnceLock;

pub(crate) struct HandleBroker {
    pub(crate) process: RawHandle,
    pub(crate) holder: Sender<()>,
}

pub(crate) static HANDLE_BROKER: OnceLock<HandleBroker> = OnceLock::new();

pub(crate) fn start_root() {
    let (ours, theirs) = channel().expect("Failed to create holder channel for handle broker");
    let broker = ManuallyDrop::new(
        handle_broker
            .spawn(theirs)
            .expect("Failed to start handle broker"),
    );
    HANDLE_BROKER
        .set(HandleBroker {
            process: broker.id(),
            holder: ours,
        })
        .ok()
        .expect("HANDLE_BROKER has already been initialized");
}

#[func]
fn handle_broker(mut holder: Receiver<()>) {
    holder
        .recv()
        .expect("Failed to receive from holder in handle broker");
    // Everyone is dead by now, there is nobody to report to
    std::process::exit(0);
}

pub(crate) fn crossmist_main(mut args: std::env::Args) -> ! {
    let handle_broker_id: RawHandle = parse_raw_handle(
        &args
            .next()
            .expect("Expected four CLI arguments for crossmist"),
    );
    let handle_broker_holder_id: RawHandle = parse_raw_handle(
        &args
            .next()
            .expect("Expected four CLI arguments for crossmist"),
    );
    let handle_tx: RawHandle = parse_raw_handle(
        &args
            .next()
            .expect("Expected four CLI arguments for crossmist"),
    );
    let handle_rx: RawHandle = parse_raw_handle(
        &args
            .next()
            .expect("Expected four CLI arguments for crossmist"),
    );

    HANDLE_BROKER
        .set(HandleBroker {
            process: handle_broker_id,
            holder: unsafe { Sender::from_raw_handle(handle_broker_holder_id) },
        })
        .ok()
        .expect("HANDLE_BROKER has already been initialized");

    enable_cloexec(handle_tx).expect("Failed to set O_CLOEXEC for the file descriptor");
    enable_cloexec(handle_rx).expect("Failed to set O_CLOEXEC for the file descriptor");

    let mut entry_rx = unsafe { Receiver::<(Vec<u8>, Vec<RawHandle>)>::from_raw_handle(handle_rx) };

    let (entry_data, entry_handles) = entry_rx
        .recv()
        .expect("Failed to read entry for crossmist")
        .expect("No entry passed");

    drop(entry_rx);

    for handle in &entry_handles {
        enable_cloexec(*handle).expect("Failed to set O_CLOEXEC for the file descriptor");
    }

    let entry_handles = entry_handles
        .into_iter()
        .map(|handle| unsafe { OwnedHandle::from_raw_handle(handle) })
        .collect();

    let mut deserializer = Deserializer::new(entry_data, entry_handles);
    let entry: Box<dyn FnOnceObject<(RawHandle,), Output = i32>> =
        unsafe { deserializer.deserialize() }.expect("Failed to deserialize entry");
    std::process::exit(entry.call_object_once((handle_tx,)))
}

fn parse_raw_handle(s: &str) -> RawHandle {
    use windows::Win32::Foundation;
    Foundation::HANDLE(s.parse::<isize>().expect("Failed to parse handle"))
}

pub(crate) fn disable_cloexec(handle: RawHandle) -> std::io::Result<()> {
    use windows::Win32::Foundation;
    unsafe {
        Foundation::SetHandleInformation(
            handle,
            Foundation::HANDLE_FLAG_INHERIT.0,
            Foundation::HANDLE_FLAG_INHERIT,
        )
        .ok()?
    };
    Ok(())
}
pub(crate) fn enable_cloexec(handle: RawHandle) -> std::io::Result<()> {
    use windows::Win32::Foundation;
    unsafe {
        Foundation::SetHandleInformation(
            handle,
            Foundation::HANDLE_FLAG_INHERIT.0,
            Foundation::HANDLE_FLAGS::default(),
        )
        .ok()?
    };
    Ok(())
}
pub(crate) fn is_cloexec(handle: RawHandle) -> std::io::Result<bool> {
    use windows::Win32::Foundation;
    let mut flags = 0u32;
    unsafe { Foundation::GetHandleInformation(handle, &mut flags as *mut u32).ok()? };
    Ok((flags & Foundation::HANDLE_FLAG_INHERIT.0) == 0)
}
