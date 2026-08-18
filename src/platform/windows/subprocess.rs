use std::os::windows::io::{BorrowedHandle, OwnedHandle, AsRawHandle, FromRawHandle, RawHandle};
use std::ffi::c_void;
use std::io::Result;
use std::sync::OnceLock;
use windows::{
    Win32::{
        Foundation::{self, HANDLE},
        System::{JobObjects, LibraryLoader, Threading},
    },
    core::{PCWSTR, PWSTR},
};

// An empty process holding in-flight handles.
pub(crate) struct Broker {
    pub(crate) process: OwnedHandle,
    job: OwnedHandle,
}

pub(crate) static HANDLE_BROKER: OnceLock<Broker> = OnceLock::new();

fn get_own_name() -> Result<Vec<u16>> {
    let mut module_name = vec![0u16; 256];
    let mut module_name_len;
    loop {
        module_name_len =
            unsafe { LibraryLoader::GetModuleFileNameW(None, &mut module_name) } as usize;
        if module_name_len == 0 {
            return Err(std::io::Error::last_os_error());
        } else if module_name_len == module_name.len() {
            module_name.resize(module_name.len() * 2, 0);
        } else {
            module_name.truncate(module_name_len + 1);
            return Ok(module_name);
        }
    }
}

pub(crate) fn start_broker() -> Result<()> {
    unsafe {
        // Create the broker in a kill-on-close job so that it doesn't linger around after every
        // user dies. The job handles acts as a keep-alive, similarly to how holding the write end
        // of the pipe keeps the reader hanging on Linux, but without wasting resources on actually
        // populating the process with an executable image.
        let job = OwnedHandle::from_raw_handle(JobObjects::CreateJobObjectW(
            None,
            PCWSTR(core::ptr::null()),
        )?.0);

        let mut limit_info = JobObjects::JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limit_info.BasicLimitInformation.LimitFlags =
            JobObjects::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        JobObjects::SetInformationJobObject(
            HANDLE(job.as_raw_handle()),
            JobObjects::JobObjectExtendedLimitInformation,
            (&raw const limit_info).cast(),
            size_of_val(&limit_info) as u32,
        )?;

        let n_attrs = 1;
        let mut size = 0;
        let _ = Threading::InitializeProcThreadAttributeList(None, n_attrs, None, &raw mut size); // errors by design according to MSDN
        let mut attrs = vec![0u8; size];
        let attrs = Threading::LPPROC_THREAD_ATTRIBUTE_LIST(attrs.as_mut_ptr().cast());
        Threading::InitializeProcThreadAttributeList(Some(attrs), n_attrs, None, &raw mut size)?;
        struct AttrsGuard(Threading::LPPROC_THREAD_ATTRIBUTE_LIST);
        impl Drop for AttrsGuard {
            fn drop(&mut self) {
                unsafe { Threading::DeleteProcThreadAttributeList(self.0) };
            }
        }
        let _attrs_guard = AttrsGuard(attrs);
        Threading::UpdateProcThreadAttribute(
            attrs,
            0,
            Threading::PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
            Some(&raw const job as *const c_void),
            size_of_val(&job),
            None,
            None,
        )?;

        let mut startup_info = Threading::STARTUPINFOEXW::default();
        startup_info.StartupInfo.cb = size_of_val(&startup_info) as u32;
        startup_info.lpAttributeList = attrs;

        let mut process_info = Threading::PROCESS_INFORMATION::default();

        let module_name = get_own_name()?;
        Threading::CreateProcessW(
            PCWSTR::from_raw(module_name.as_ptr()),
            None,
            None,
            None,
            false,
            Threading::CREATE_SUSPENDED | Threading::EXTENDED_STARTUPINFO_PRESENT,
            None,
            None,
            (&raw const startup_info).cast(),
            &raw mut process_info,
        )?;
        let process = OwnedHandle::from_raw_handle(process_info.hProcess.0);
        Foundation::CloseHandle(process_info.hThread)?;

        HANDLE_BROKER
            .set(Broker { process, job })
            .ok()
            .expect("broker already initialized");
        Ok(())
    }
}

pub(crate) fn set_broker(process: OwnedHandle, job: OwnedHandle) {
    HANDLE_BROKER
        .set(Broker { process, job })
        .ok()
        .expect("broker already initialized");
}

pub(crate) unsafe fn _spawn_child<'a>(child_handle: BorrowedHandle<'a>) -> Result<OwnedHandle> {
    unsafe {
        let broker = HANDLE_BROKER.get().expect("broker not initialized");

        // Pass the handles as visible in the current process, and let the child duplicate them into
        // itself manually. Every other way, like using inheritance, duplicating into a suspended
        // child, or setting the parent to the broker process, is unfortunately inherently racy.
        let creation_time = get_creation_time(Threading::GetCurrentProcess().0)?;
        let mut cmd_line: Vec<u16> = format!(
            "_crossmist_ {} {} {} {} {}\0",
            Threading::GetCurrentProcessId(),
            creation_time,
            broker.process.as_raw_handle().addr(),
            broker.job.as_raw_handle().addr(),
            child_handle.as_raw_handle().addr(),
        )
        .encode_utf16()
        .collect();

        let mut startup_info = Threading::STARTUPINFOW::default();
        startup_info.cb = size_of_val(&startup_info) as u32;

        let mut process_info = Threading::PROCESS_INFORMATION::default();

        let module_name = get_own_name()?;
        Threading::CreateProcessW(
            PCWSTR::from_raw(module_name.as_ptr()),
            Some(PWSTR::from_raw(cmd_line.as_mut_ptr())),
            None,
            None,
            true,
            Threading::INHERIT_PARENT_AFFINITY,
            None,
            None,
            &raw const startup_info,
            &raw mut process_info,
        )?;

        Foundation::CloseHandle(process_info.hThread)?;
        Ok(OwnedHandle::from_raw_handle(process_info.hProcess.0))
    }
}

pub(crate) unsafe fn get_creation_time(process: RawHandle) -> Result<u64> {
    let mut creation_time = Default::default();
    let mut ignore = Default::default();
    unsafe {
        Threading::GetProcessTimes(
            HANDLE(process),
            &raw mut creation_time,
            &raw mut ignore,
            &raw mut ignore,
            &raw mut ignore,
        )
    }?;
    Ok(creation_time.dwLowDateTime as u64 | (creation_time.dwHighDateTime as u64) << 32)
}
