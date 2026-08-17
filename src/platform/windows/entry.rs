use crate::{
    Deserializer, Receiver, Sender,
    asynchronous::handle_entry,
    channel,
    handles::{
        AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, IntoRawHandle, OwnedHandle, RawHandle,
    },
};
use std::default::Default;
use std::sync::OnceLock;
use windows::Win32::Foundation;

pub(crate) struct HandleBroker {
    pub(crate) process: OwnedHandle,
    pub(crate) holder: Sender<()>,
}

pub(crate) static HANDLE_BROKER: OnceLock<HandleBroker> = OnceLock::new();

pub(crate) fn start_root() {
    let (ours, theirs) = channel().expect("Failed to create holder channel for handle broker");
    let broker = handle_broker
        .spawn(theirs)
        .expect("Failed to start handle broker");
    HANDLE_BROKER
        .set(HandleBroker {
            process: broker.0.proc_handle,
            holder: ours,
        })
        .ok()
        .expect("HANDLE_BROKER has already been initialized");
}

#[crossmist::func]
fn handle_broker(mut holder: Receiver<()>) {
    holder
        .recv()
        .expect("Failed to receive from holder in handle broker");
    // Everyone is dead by now, there is nobody to report to
    std::process::exit(0);
}

pub(crate) fn crossmist_main(mut args: std::env::Args) -> ! {
    let [
        handle_broker_id,
        handle_broker_holder_id,
        handle_tx,
        handle_rx,
    ] = core::array::from_fn(|_| unsafe {
        parse_handle(
            &args
                .next()
                .expect("Expected four CLI arguments for crossmist"),
        )
    });

    HANDLE_BROKER
        .set(HandleBroker {
            process: handle_broker_id,
            holder: unsafe { Sender::from_raw_handle(handle_broker_holder_id.into_raw_handle()) },
        })
        .ok()
        .expect("HANDLE_BROKER has already been initialized");

    enable_cloexec(handle_tx.as_handle()).expect("Failed to set O_CLOEXEC for the file descriptor");
    enable_cloexec(handle_rx.as_handle()).expect("Failed to set O_CLOEXEC for the file descriptor");

    let mut entry_rx = unsafe {
        Receiver::<(Vec<u8>, Vec<RawHandle>)>::from_raw_handle(handle_rx.into_raw_handle())
    };

    let (entry_data, entry_handles) = entry_rx
        .recv()
        .expect("Failed to read entry for crossmist")
        .expect("No entry passed");

    drop(entry_rx);

    let entry_handles = entry_handles
        .into_iter()
        .map(|handle| unsafe { OwnedHandle::from_raw_handle(handle) })
        .collect::<Vec<_>>();

    for handle in &entry_handles {
        enable_cloexec(handle.as_handle())
            .expect("Failed to set O_CLOEXEC for the file descriptor");
    }

    handle_entry(
        Deserializer::new(entry_data, entry_handles),
        handle_tx.as_raw_handle(),
    );
}

unsafe fn parse_handle(s: &str) -> OwnedHandle {
    let handle = Foundation::HANDLE(core::ptr::without_provenance_mut(
        s.parse().expect("Failed to parse handle"),
    ));
    unsafe { OwnedHandle::from_raw_handle(handle) }
}

pub(crate) fn disable_cloexec(handle: BorrowedHandle<'_>) -> std::io::Result<()> {
    unsafe {
        Foundation::SetHandleInformation(
            handle.as_raw_handle(),
            Foundation::HANDLE_FLAG_INHERIT.0,
            Foundation::HANDLE_FLAG_INHERIT,
        )
    }?;
    Ok(())
}
pub(crate) fn enable_cloexec(handle: BorrowedHandle<'_>) -> std::io::Result<()> {
    unsafe {
        Foundation::SetHandleInformation(
            handle.as_raw_handle(),
            Foundation::HANDLE_FLAG_INHERIT.0,
            Foundation::HANDLE_FLAGS::default(),
        )
    }?;
    Ok(())
}
pub(crate) fn is_cloexec(handle: BorrowedHandle<'_>) -> std::io::Result<bool> {
    let mut flags = 0u32;
    unsafe { Foundation::GetHandleInformation(handle.as_raw_handle(), &mut flags as *mut u32) }?;
    Ok((flags & Foundation::HANDLE_FLAG_INHERIT.0) == 0)
}
