use crate::{
    channel, func,
    handles::{FromRawHandle, OwnedHandle, RawHandle},
    Deserializer, FnOnceObject, Receiver, Sender,
};
use lazy_static::lazy_static;
use std::default::Default;
use std::sync::RwLock;

lazy_static! {
    pub(crate) static ref HANDLE_BROKER: RwLock<RawHandle> = RwLock::new(Default::default());
    pub(crate) static ref HANDLE_BROKER_HOLDER: RwLock<Option<Sender<()>>> = RwLock::new(None);
}

pub(crate) fn start_root() {
    let (ours, theirs) = channel().expect("Failed to create holder channel for handle broker");
    let id = Box::leak(Box::new(
        handle_broker
            .spawn(theirs)
            .expect("Failed to start handle broker"),
    ))
    .id();
    *HANDLE_BROKER
        .write()
        .expect("Failed to acquire write access to HANDLE_BROKER") = id;
    *HANDLE_BROKER_HOLDER
        .write()
        .expect("Failed to acquire write access to HANDLE_BROKER_HOLDER") = Some(ours);
}

#[func]
fn handle_broker(mut holder: Receiver<()>) -> ! {
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

    *HANDLE_BROKER
        .write()
        .expect("Failed to acquire write access to HANDLE_BROKER") = handle_broker_id;
    *HANDLE_BROKER_HOLDER
        .write()
        .expect("Failed to acquire write access to HANDLE_BROKER_HOLDER") =
        Some(unsafe { Sender::from_raw_handle(handle_broker_holder_id) });

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
        unsafe { deserializer.deserialize() };
    std::process::exit(entry(handle_tx))
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
