use crate::{
    Deserializer, Object, Serializer,
    handles::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle},
    subprocess::HANDLE_BROKER,
};
use std::default::Default;
use std::fs::File;
use std::io::Result;
use windows::Win32::{
    Foundation,
    Security::{self, Authorization},
    Storage::FileSystem,
    System::{Pipes, SystemServices, Threading},
};
use windows::core::{PCSTR, PSTR};

pub(crate) fn socketpair() -> Result<(File, File)> {
    // Set a security descriptor for the pipe so that no other user can connect to it. This is
    // not necessary for security, since we only allow one client and validate that our own
    // connection went through, but it guarantees that a malicious program doesn't race us to
    // connection and cause a failure.
    let mut sd = Security::SECURITY_DESCRIPTOR::default();
    let sd = Security::PSECURITY_DESCRIPTOR((&raw mut sd).cast());
    unsafe {
        Security::InitializeSecurityDescriptor(sd, SystemServices::SECURITY_DESCRIPTOR_REVISION)
    }?;

    let ea = Authorization::EXPLICIT_ACCESS_A {
        grfAccessPermissions: (Foundation::GENERIC_READ | Foundation::GENERIC_WRITE).0,
        grfAccessMode: Authorization::SET_ACCESS,
        grfInheritance: Security::NO_INHERITANCE,
        Trustee: Authorization::TRUSTEE_A {
            pMultipleTrustee: core::ptr::null_mut(),
            MultipleTrusteeOperation: Authorization::NO_MULTIPLE_TRUSTEE,
            TrusteeForm: Authorization::TRUSTEE_IS_NAME,
            TrusteeType: Authorization::TRUSTEE_IS_USER,
            ptstrName: PSTR(c"CURRENT_USER".as_ptr() as _),
        },
    };

    struct Acl(*mut Security::ACL); // stored on the local heap
    impl Drop for Acl {
        fn drop(&mut self) {
            unsafe { Foundation::LocalFree(Some(Foundation::HLOCAL(self.0.cast()))) };
        }
    }
    let mut acl = Acl(core::ptr::null_mut());
    unsafe {
        Authorization::SetEntriesInAclA(Some(core::slice::from_ref(&ea)), None, &raw mut acl.0)
    }
    .ok()?;
    unsafe { Security::SetSecurityDescriptorDacl(sd, true, Some(acl.0), false) }?;

    let sa = Security::SECURITY_ATTRIBUTES {
        nLength: size_of::<Security::SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: false.into(),
    };

    let mut buf = [0u8; 32];
    let _ = unsafe { Security::Cryptography::ProcessPrng(&mut buf) }; // documented to return TRUE
    let mut path = br"\\.\pipe\crossmist-".to_vec();
    path.extend(buf.into_iter().map(|c| b'a' + (c & 15)));
    path.push(0);
    let path = PCSTR(path.as_ptr());

    let tx = unsafe {
        Pipes::CreateNamedPipeA(
            path,
            FileSystem::PIPE_ACCESS_DUPLEX | FileSystem::FILE_FLAG_FIRST_PIPE_INSTANCE,
            Pipes::PIPE_TYPE_MESSAGE
                | Pipes::PIPE_READMODE_MESSAGE
                | Pipes::PIPE_WAIT
                | Pipes::PIPE_REJECT_REMOTE_CLIENTS,
            1,    // 1 instance so that the pipe name cannot be reused to possibly cause issues
            2048, // buffer size
            2048,
            0,
            Some(&raw const sa),
        )
    }?;
    let tx = unsafe { File::from_raw_handle(tx) };

    let rx = unsafe {
        FileSystem::CreateFileA(
            path,
            (Foundation::GENERIC_READ | Foundation::GENERIC_WRITE).0,
            FileSystem::FILE_SHARE_MODE(0),
            None,
            FileSystem::OPEN_EXISTING,
            // `SECURITY_SQOS_PRESENT` should be passed even though we can be the only process on
            // the other side, since the process holding `tx` may have different permissions from
            // the process holding `rx` if a stream is sent across processes.
            FileSystem::FILE_ATTRIBUTE_NORMAL | FileSystem::SECURITY_SQOS_PRESENT,
            None,
        )
    }?;
    let rx = unsafe { File::from_raw_handle(rx) };

    Ok((tx, rx))
}

pub(crate) fn serialize_with_handles<T: Object>(value: T) -> Result<Vec<u8>> {
    let mut s = Serializer::new();
    s.serialize(value);

    let (data, handles) = s.into_parts();
    let mut remote_handles = Vec::new();
    if !handles.is_empty() {
        let broker = HANDLE_BROKER
            .get()
            .expect("broker has not been initialized");

        for handle in handles {
            let mut remote_handle: RawHandle = Default::default();
            unsafe {
                Foundation::DuplicateHandle(
                    Threading::GetCurrentProcess(),
                    handle.as_raw_handle(),
                    broker.process.as_raw_handle(),
                    &mut remote_handle,
                    0,
                    false,
                    Foundation::DUPLICATE_SAME_ACCESS,
                )?;
            }
            remote_handles.push(remote_handle);
        }
    }

    let mut s1 = Serializer::new();
    s1.serialize(remote_handles);
    s1.write(&data);
    Ok(s1.into_parts().0)
}

pub(crate) unsafe fn deserialize_with_handles<T: Object>(serialized: Vec<u8>) -> Result<T> {
    let mut d = Deserializer::new(serialized, Vec::new());
    let remote_handles: Vec<RawHandle> = unsafe { d.deserialize() };
    let serialized_contents: Vec<u8> = Vec::from(d.get_rest());

    let mut handles = Vec::new();
    if !remote_handles.is_empty() {
        let broker = HANDLE_BROKER
            .get()
            .expect("broker has not been initialized");

        for remote_handle in remote_handles {
            let mut handle: RawHandle = Default::default();
            unsafe {
                Foundation::DuplicateHandle(
                    broker.process.as_raw_handle(),
                    remote_handle,
                    Threading::GetCurrentProcess(),
                    &mut handle,
                    0,
                    false,
                    Foundation::DUPLICATE_CLOSE_SOURCE | Foundation::DUPLICATE_SAME_ACCESS,
                )?;
            }
            let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
            handles.push(handle);
        }
    }

    Ok(unsafe { Deserializer::new(serialized_contents, handles).deserialize() })
}
