use crate::{
    entry,
    handles::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle},
};
use std::ffi::c_void;
use std::io::Result;
use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation,
        System::{LibraryLoader, Threading},
    },
};

pub(crate) unsafe fn _spawn_child(
    child_tx: RawHandle,
    child_rx: RawHandle,
    inherited_handles: &[RawHandle],
) -> Result<OwnedHandle> {
    let mut inherited_handles = inherited_handles.to_vec();
    inherited_handles.push(child_tx);
    inherited_handles.push(child_rx);

    let handle_broker = *entry::HANDLE_BROKER
        .read()
        .expect("Failed to acquire read access to HANDLE_BROKER");
    if !handle_broker.is_invalid() {
        inherited_handles.push(handle_broker);
    }
    if let Some(sender) = entry::HANDLE_BROKER_HOLDER
        .read()
        .expect("Failed to acquire read access to HANDLE_BROKER_HOLDER")
        .as_ref()
    {
        inherited_handles.push(sender.as_raw_handle());
    }

    let mut module_name = vec![0u16; 256];
    let mut module_name_len;
    loop {
        module_name_len = LibraryLoader::GetModuleFileNameW(None, &mut module_name) as usize;
        if module_name_len == 0 {
            return Err(std::io::Error::last_os_error());
        } else if module_name_len == module_name.len() {
            module_name.resize(module_name.len() * 2, 0);
        } else {
            module_name.truncate(module_name_len + 1);
            break;
        }
    }

    let mut cmd_line: Vec<u16> = format!(
        "_crossmist_ {} {} {} {}\0",
        entry::HANDLE_BROKER
            .read()
            .expect("Failed to acquire read access to HANDLE_BROKER")
            .0,
        entry::HANDLE_BROKER_HOLDER
            .read()
            .expect("Failed to acquire read access to HANDLE_BROKER_HOLDER")
            .as_ref()
            .map(|sender| sender.as_raw_handle().0)
            .unwrap_or(0),
        child_tx.0,
        child_rx.0
    )
    .encode_utf16()
    .collect();

    let n_attrs = 1;
    let mut size = 0;
    Threading::InitializeProcThreadAttributeList(
        Threading::LPPROC_THREAD_ATTRIBUTE_LIST::default(),
        n_attrs,
        0,
        &mut size as *mut usize,
    );
    let mut attrs = vec![0u8; size];
    let attrs = Threading::LPPROC_THREAD_ATTRIBUTE_LIST(attrs.as_mut_ptr() as *mut c_void);
    Threading::InitializeProcThreadAttributeList(attrs, n_attrs, 0, &mut size as *mut usize)
        .ok()?;
    Threading::UpdateProcThreadAttribute(
        attrs,
        0,
        Threading::PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
        inherited_handles.as_ptr() as *const c_void,
        inherited_handles.len() * std::mem::size_of::<RawHandle>(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    )
    .ok()?;

    let mut startup_info = Threading::STARTUPINFOEXW::default();
    startup_info.StartupInfo.cb = std::mem::size_of::<Threading::STARTUPINFOEXW>() as u32;
    startup_info.lpAttributeList = attrs;

    let mut process_info = Threading::PROCESS_INFORMATION::default();

    let mut enabled_handles = Vec::new();
    for &handle in &inherited_handles {
        if entry::is_cloexec(handle)? {
            enabled_handles.push(handle);
            entry::disable_cloexec(handle)?;
        }
    }

    let res = Threading::CreateProcessW(
        PCWSTR::from_raw(module_name.as_ptr()),
        PWSTR::from_raw(cmd_line.as_mut_ptr()),
        std::ptr::null(),
        std::ptr::null(),
        true,
        Threading::EXTENDED_STARTUPINFO_PRESENT | Threading::INHERIT_PARENT_AFFINITY,
        std::ptr::null(),
        None,
        &startup_info as *const Threading::STARTUPINFOEXW as *const Threading::STARTUPINFOW,
        &mut process_info as *mut Threading::PROCESS_INFORMATION,
    );

    for handle in enabled_handles {
        entry::enable_cloexec(handle)?;
    }

    res.ok()?;

    Foundation::CloseHandle(process_info.hThread);
    Ok(OwnedHandle::from_raw_handle(process_info.hProcess))
}
