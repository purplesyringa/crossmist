use std::ffi::{OsStr, c_void};
use std::io::Result;
use std::os::windows::{
    ffi::OsStrExt,
    io::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle, RawHandle},
};
use std::sync::OnceLock;
use windows::{
    Wdk::System::Threading::{NtQueryInformationProcess, ProcessBasicInformation},
    Win32::{
        Foundation::{self, HANDLE},
        System::{Diagnostics::Debug, JobObjects, LibraryLoader, Threading},
    },
    core::{PCWSTR, PWSTR},
};

// An empty process holding in-flight handles.
pub(crate) struct Broker {
    pub(crate) process: OwnedHandle,
    pub(crate) pid: u32,
    job: OwnedHandle,
}

pub(crate) static HANDLE_BROKER: OnceLock<Broker> = OnceLock::new();

#[derive(Clone, Copy, Default)]
struct InitData {
    broker_process: HANDLE,
    broker_pid: u32,
    broker_job: HANDLE,
    child_handle: HANDLE,
}

static mut INIT_DATA: InitData = unsafe { core::mem::zeroed() };

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

unsafe fn set_job_flags(job: RawHandle, flags: JobObjects::JOB_OBJECT_LIMIT) -> Result<()> {
    let mut limit_info = JobObjects::JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limit_info.BasicLimitInformation.LimitFlags = flags;
    unsafe {
        JobObjects::SetInformationJobObject(
            HANDLE(job),
            JobObjects::JobObjectExtendedLimitInformation,
            (&raw const limit_info).cast(),
            size_of_val(&limit_info) as u32,
        )
    }?;
    Ok(())
}

fn spawn_suspended_in_job(
    cmd_line: Option<&OsStr>,
) -> Result<(OwnedHandle, OwnedHandle, u32, OwnedHandle)> {
    unsafe {
        let job = OwnedHandle::from_raw_handle(
            JobObjects::CreateJobObjectW(None, PCWSTR(core::ptr::null()))?.0,
        );
        set_job_flags(
            job.as_raw_handle(),
            JobObjects::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
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
        let mut cmd_line = cmd_line.map(|cmd_line| {
            let mut wide: Vec<u16> = cmd_line.encode_wide().collect();
            wide.push(0);
            wide
        });
        Threading::CreateProcessW(
            PCWSTR::from_raw(module_name.as_ptr()),
            // `CreateProcessW` modifies `cmd_line`.
            cmd_line
                .as_mut()
                .map(|cmd_line| PWSTR::from_raw(cmd_line.as_mut_ptr())),
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
        let thread = OwnedHandle::from_raw_handle(process_info.hThread.0);
        let pid = process_info.dwProcessId;

        Ok((process, thread, pid, job))
    }
}

pub(crate) fn start_broker() -> Result<()> {
    // Create the broker in a kill-on-close job so that it doesn't linger around after every user
    // dies. The job handles acts as a keep-alive, similarly to how holding the write end of the
    // pipe keeps the reader hanging on Linux, but without wasting resources on actually populating
    // the process with an executable image.
    let (process, _, pid, job) = spawn_suspended_in_job(None)?;
    HANDLE_BROKER
        .set(Broker { process, pid, job })
        .ok()
        .expect("broker already initialized");
    Ok(())
}

pub(crate) unsafe fn load_init_handles() -> OwnedHandle {
    let init_data = unsafe { INIT_DATA };
    let [process, job, handle] = [
        init_data.broker_process,
        init_data.broker_job,
        init_data.child_handle,
    ]
    .map(|handle| unsafe { OwnedHandle::from_raw_handle(handle.0) });
    let pid = init_data.broker_pid;
    HANDLE_BROKER
        .set(Broker { process, pid, job })
        .ok()
        .expect("broker already initialized");
    handle
}

unsafe fn get_peb(process: RawHandle) -> Result<*const Threading::PEB> {
    // Communicate the handles via memory injection, since we have no other way of interacting
    // with the child process yet.
    let mut proc = Threading::PROCESS_BASIC_INFORMATION::default();
    unsafe {
        NtQueryInformationProcess(
            HANDLE(process),
            ProcessBasicInformation,
            (&raw mut proc).cast(),
            // avoid `size_of_val` due to aliasing
            size_of::<Threading::PROCESS_BASIC_INFORMATION>() as u32,
            core::ptr::null_mut(),
        )
    }
    .ok()?;
    Ok(proc.PebBaseAddress)
}

pub(crate) unsafe fn _spawn_child<'a>(child_handle: BorrowedHandle<'a>) -> Result<OwnedHandle> {
    unsafe {
        let broker = HANDLE_BROKER.get().expect("broker not initialized");

        // Create the child suspended and duplicate handles into it, since using normal handle
        // inheritance is broken [1], and setting a custom parent [2] is broken on Wine and requires
        // spawning another process.
        // [1]: https://github.com/rust-lang/rust/issues/161158
        // [2]: https://devblogs.microsoft.com/oldnewthing/20260511-00/?p=112313

        // Create the child in a (temporarily) kill-on-close job so that it doesn't remain in a coma
        // if we die before completing the startup.
        let (process, thread, _, job) = spawn_suspended_in_job(Some(OsStr::new("_crossmist_")))?;

        let mut init_data = InitData {
            broker_pid: broker.pid,
            ..Default::default()
        };
        for (handle, remote_handle) in [
            (broker.process.as_handle(), &mut init_data.broker_process),
            (broker.job.as_handle(), &mut init_data.broker_job),
            (child_handle, &mut init_data.child_handle),
        ] {
            Foundation::DuplicateHandle(
                Threading::GetCurrentProcess(),
                HANDLE(handle.as_raw_handle()),
                HANDLE(process.as_raw_handle()),
                remote_handle,
                0,
                false,
                Foundation::DUPLICATE_SAME_ACCESS,
            )?;
        }

        // Communicate the handles via memory injection, since we have no other way of interacting
        // with the child process yet.
        let peb = get_peb(Threading::GetCurrentProcess().0)?;
        let image_base_address = (*peb).Reserved3[1].addr();
        let remote_peb = get_peb(process.as_raw_handle())?;
        let mut remote_image_base_address = 0usize;
        Debug::ReadProcessMemory(
            HANDLE(process.as_raw_handle()),
            (&raw const (*remote_peb).Reserved3[1]).cast(),
            (&raw mut remote_image_base_address).cast(),
            size_of::<usize>(), // avoid `size_of_val` due to aliasing
            None,
        )?;
        let offset = remote_image_base_address.wrapping_sub(image_base_address);
        let remote_init_data = (&raw mut INIT_DATA).byte_add(offset);
        Debug::WriteProcessMemory(
            HANDLE(process.as_raw_handle()),
            remote_init_data.cast(),
            (&raw const init_data).cast(),
            size_of_val(&init_data),
            None,
        )?;

        if Threading::ResumeThread(HANDLE(thread.as_raw_handle())) == u32::MAX {
            return Err(std::io::Error::last_os_error());
        }

        // Drop kill-on-close to let the process live independently from us, and also let it break
        // away to avoid heavily nested jobs.
        set_job_flags(
            job.as_raw_handle(),
            JobObjects::JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK,
        )?;

        Ok(process)
    }
}
