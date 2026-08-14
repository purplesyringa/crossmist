use crate::{
    asynchronous::AsyncStream,
    entry,
    handles::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle, RawHandle},
};
use std::ffi::c_void;
use std::io::Result;
use windows::{
    Win32::{
        Foundation,
        System::{LibraryLoader, Threading},
    },
    core::{PCWSTR, PWSTR},
};

pub(crate) unsafe fn _spawn_child<'a>(
    child_tx: BorrowedHandle<'a>,
    child_rx: BorrowedHandle<'a>,
    mut inherited_handles: Vec<BorrowedHandle<'a>>,
) -> Result<OwnedHandle> {
    unsafe {
        inherited_handles.push(child_tx);
        inherited_handles.push(child_rx);

        let (broker_process, holder_handle) = match entry::HANDLE_BROKER.get() {
            Some(handle_broker) => {
                inherited_handles.push(handle_broker.process.as_handle());
                inherited_handles.push(handle_broker.holder.0.fd.as_handle());
                (
                    handle_broker.process.as_raw_handle(),
                    handle_broker.holder.as_raw_handle(),
                )
            }
            None => {
                // HANDLE_BROKER is not initialized before the broker itself is started
                (
                    Foundation::INVALID_HANDLE_VALUE,
                    Foundation::INVALID_HANDLE_VALUE,
                )
            }
        };

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
            broker_process.0.addr(),
            holder_handle.0.addr(),
            child_tx.as_raw_handle().0.addr(),
            child_rx.as_raw_handle().0.addr(),
        )
        .encode_utf16()
        .collect();

        let n_attrs = 1;
        let mut size = 0;
        Threading::InitializeProcThreadAttributeList(None, n_attrs, None, &mut size as *mut usize)?;
        let mut attrs = vec![0u8; size];
        let attrs = Threading::LPPROC_THREAD_ATTRIBUTE_LIST(attrs.as_mut_ptr() as *mut c_void);
        Threading::InitializeProcThreadAttributeList(
            Some(attrs),
            n_attrs,
            None,
            &mut size as *mut usize,
        )?;
        Threading::UpdateProcThreadAttribute(
            attrs,
            0,
            Threading::PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            Some(inherited_handles.as_ptr() as *const c_void),
            inherited_handles.len() * std::mem::size_of::<RawHandle>(),
            None,
            None,
        )?;

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
            Some(PWSTR::from_raw(cmd_line.as_mut_ptr())),
            None,
            None,
            true,
            Threading::EXTENDED_STARTUPINFO_PRESENT | Threading::INHERIT_PARENT_AFFINITY,
            None,
            None,
            &startup_info as *const Threading::STARTUPINFOEXW as *const Threading::STARTUPINFOW,
            &mut process_info as *mut Threading::PROCESS_INFORMATION,
        );

        for handle in enabled_handles {
            entry::enable_cloexec(handle)?;
        }

        res?;

        Foundation::CloseHandle(process_info.hThread)?;
        Ok(OwnedHandle::from_raw_handle(process_info.hProcess))
    }
}
