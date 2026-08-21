use std::ffi::OsStr;
use std::io::Result;
use std::os::windows::{
    ffi::OsStrExt,
    io::{AsRawHandle, AsRawSocket, BorrowedSocket, FromRawHandle, OwnedHandle, RawHandle},
};
use std::sync::OnceLock;
use windows::{
    Wdk::System::Threading as NtThreading,
    Win32::{
        Foundation,
        System::{JobObjects, LibraryLoader, Threading},
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

fn create_job() -> Result<OwnedHandle> {
    let job = unsafe {
        OwnedHandle::from_raw_handle(
            JobObjects::CreateJobObjectW(None, PCWSTR(core::ptr::null()))?.0,
        )
    };
    let mut limit_info = JobObjects::JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limit_info.BasicLimitInformation.LimitFlags = JobObjects::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    unsafe {
        JobObjects::SetInformationJobObject(
            Foundation::HANDLE(job.as_raw_handle()),
            JobObjects::JobObjectExtendedLimitInformation,
            (&raw const limit_info).cast(),
            size_of_val(&limit_info) as u32,
        )
    }?;
    Ok(job)
}

fn spawn(
    cmd_line: Option<&OsStr>,
    suspended: bool,
    job: Option<&OwnedHandle>,
) -> Result<(OwnedHandle, OwnedHandle, u32)> {
    unsafe {
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
        if let Some(job) = job {
            Threading::UpdateProcThreadAttribute(
                attrs,
                0,
                Threading::PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                Some(core::ptr::from_ref(job).cast()),
                size_of_val(job),
                None,
                None,
            )?;
        }

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
            if suspended {
                Threading::CREATE_SUSPENDED
            } else {
                Threading::PROCESS_CREATION_FLAGS(0)
            } | Threading::EXTENDED_STARTUPINFO_PRESENT,
            None,
            None,
            (&raw const startup_info).cast(),
            &raw mut process_info,
        )?;
        let process = OwnedHandle::from_raw_handle(process_info.hProcess.0);
        let thread = OwnedHandle::from_raw_handle(process_info.hThread.0);
        let pid = process_info.dwProcessId;

        Ok((process, thread, pid))
    }
}

pub(crate) fn start_broker() -> Result<()> {
    // Create the broker in a kill-on-close job so that it doesn't linger around after every user
    // dies. The job handles acts as a keep-alive, similarly to how holding the write end of the
    // pipe keeps the reader hanging on Linux, but without wasting resources on actually populating
    // the process with an executable image.
    let job = create_job()?;
    let (process, _, pid) = spawn(None, true, Some(&job))?;
    HANDLE_BROKER
        .set(Broker { process, pid, job })
        .ok()
        .expect("broker already initialized");
    Ok(())
}

pub(crate) fn set_broker(process: OwnedHandle, pid: u32, job: OwnedHandle) {
    HANDLE_BROKER
        .set(Broker { process, pid, job })
        .ok()
        .expect("broker already initialized");
}

pub(crate) unsafe fn _spawn_child<'a>(child_socket: BorrowedSocket<'a>) -> Result<OwnedHandle> {
    unsafe {
        let broker = HANDLE_BROKER.get().expect("broker not initialized");

        // Using normal handle inheritance is broken [1], and setting a custom parent [2] is broken
        // on Wine and requires spawning another process, so let the child steal handles from us.
        // [1]: https://github.com/rust-lang/rust/issues/161158
        // [2]: https://devblogs.microsoft.com/oldnewthing/20260511-00/?p=112313
        let seqnum = get_sequence_number(Threading::GetCurrentProcess().0)?;
        let cmd_line = format!(
            "_crossmist_ {} {} {} {} {} {}\0",
            Threading::GetCurrentProcessId(),
            seqnum,
            broker.process.as_raw_handle().addr(),
            broker.job.as_raw_handle().addr(),
            child_socket.as_raw_socket(),
            broker.pid,
        );

        let (process, _, _) = spawn(Some(OsStr::new(&cmd_line)), false, None)?;
        Ok(process)
    }
}

pub(crate) unsafe fn get_sequence_number(process: RawHandle) -> Result<u64> {
    #[allow(nonstandard_style)]
    let ProcessSequenceNumber = NtThreading::PROCESSINFOCLASS(92);

    let mut seqnum = 0u64;
    if unsafe {
        NtThreading::NtQueryInformationProcess(
            Foundation::HANDLE(process),
            ProcessSequenceNumber,
            (&raw mut seqnum).cast(),
            size_of::<u64>() as u32,
            core::ptr::null_mut(),
        )
    }
    .is_ok()
    {
        return Ok(seqnum);
    }

    // ProcessSequenceNumber doesn't seem to work in WoW64 programs, but
    // ProcessTelemetryIdInformation still works.
    // https://learn.microsoft.com/en-us/windows/win32/devnotes/process_telemetry_id_information_type
    #[allow(nonstandard_style, unused)]
    #[derive(Default)]
    #[repr(C)]
    struct PROCESS_TELEMETRY_ID_INFORMATION {
        HeaderSize: u32,
        ProcessId: u32,
        ProcessStartKey: u64,
        CreateTime: u64,
        CreateInterruptTime: u64,
        CreateUnbiasedInterruptTime: u64,
        ProcessSequenceNumber: u64,
        SessionCreateTime: u64,
        SessionId: u32,
        BootId: u32,
        ImageChecksum: u32,
        ImageTimeDateStamp: u32,
        UserSidOffset: u32,
        ImagePathOffset: u32,
        PackageNameOffset: u32,
        RelativeAppNameOffset: u32,
        CommandLineOffset: u32,
    }
    let mut telemetry = PROCESS_TELEMETRY_ID_INFORMATION::default();
    let res = unsafe {
        NtThreading::NtQueryInformationProcess(
            Foundation::HANDLE(process),
            NtThreading::ProcessTelemetryIdInformation,
            (&raw mut telemetry).cast(),
            size_of::<PROCESS_TELEMETRY_ID_INFORMATION>() as u32,
            core::ptr::null_mut(),
        )
    };
    // The definition of `PROCESS_TELEMETRY_ID_INFORMATION` in devnotes seems to be incomplete, make
    // sure to allow truncated data.
    if res != Foundation::STATUS_BUFFER_OVERFLOW {
        res.ok()?;
    }
    Ok(telemetry.ProcessSequenceNumber)
}
