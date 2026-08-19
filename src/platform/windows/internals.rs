use crate::{Deserializer, Object, Serializer, subprocess::HANDLE_BROKER};
use std::io::{Error, ErrorKind, Result};
use std::net::TcpStream;
use std::os::windows::io::{
    AsRawHandle, AsRawSocket, FromRawHandle, FromRawSocket, IntoRawHandle, OwnedHandle,
    OwnedSocket, RawHandle, RawSocket,
};
use std::path::PathBuf;
use std::sync::LazyLock;
use windows::Win32::{
    Foundation::{self, HANDLE},
    Networking::WinSock,
    Security::Cryptography,
    System::Threading,
};

pub(crate) fn socketpair() -> Result<(TcpStream, TcpStream)> {
    loop {
        if let Some(out) = try_socketpair()? {
            return Ok(out);
        }
    }
}

pub(crate) fn try_socketpair() -> Result<Option<(TcpStream, TcpStream)>> {
    // Achieves two purposes: initializes WSA and generates a temporary directory path once, since
    // Wine spams FIXMEs on `GetTempPath2W` and I don't want to read them.
    static INIT: LazyLock<core::result::Result<PathBuf, WinSock::WSA_ERROR>> =
        LazyLock::new(|| {
            let mut data = WinSock::WSADATA::default();
            if unsafe { WinSock::WSAStartup(0x0202, &raw mut data) } != 0 {
                return Err(unsafe { WinSock::WSAGetLastError() });
            }
            Ok(std::env::temp_dir())
        });
    let temp_dir = match INIT.as_deref() {
        Ok(temp_dir) => temp_dir,
        Err(err) => return Err(Error::from_raw_os_error(err.0)),
    };

    let mut rng = [0u8; 32];
    let _ = unsafe { Cryptography::ProcessPrng(&mut rng) }; // documented to return TRUE

    let mut name = "crossmist-".to_string();
    for c in rng {
        name.push((b'a' + (c & 15)) as char);
    }
    let path = temp_dir.join(name);
    let path_bytes = path.as_os_str().as_encoded_bytes();

    let mut addr = WinSock::SOCKADDR_UN {
        sun_family: WinSock::ADDRESS_FAMILY(WinSock::AF_UNIX),
        ..Default::default()
    };
    if path_bytes.len() > addr.sun_path.len() {
        return Err(Error::from(ErrorKind::InvalidFilename));
    }
    for (dst, src) in addr.sun_path.iter_mut().zip(path_bytes) {
        *dst = *src as i8;
    }

    unsafe {
        let raw_server = WinSock::socket(WinSock::AF_UNIX as i32, WinSock::SOCK_STREAM, 0)?;
        let _server = OwnedSocket::from_raw_socket(raw_server.0 as RawSocket);
        if WinSock::bind(
            raw_server,
            (&raw const addr).cast(),
            size_of_val(&addr) as i32,
        ) != 0
        {
            return Err(Error::from_raw_os_error(WinSock::WSAGetLastError().0));
        }
        struct SockGuard(PathBuf);
        impl Drop for SockGuard {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let _guard = SockGuard(path);
        if WinSock::listen(raw_server, 1) != 0 {
            return Err(Error::from_raw_os_error(WinSock::WSAGetLastError().0));
        }

        let raw_client = WinSock::socket(WinSock::AF_UNIX as i32, WinSock::SOCK_STREAM, 0)?;
        let client = OwnedSocket::from_raw_socket(raw_client.0 as RawSocket);
        let connect_result = WinSock::connect(
            raw_client,
            (&raw const addr).cast(),
            size_of_val(&addr) as i32,
        );
        if connect_result != 0 {
            let err = Error::from_raw_os_error(WinSock::WSAGetLastError().0);
            // If someone races us to connection, the backlog will be full and `connect` will return
            // `ConnectionRefused`, which we can detect and retry.
            if err.kind() == ErrorKind::ConnectionRefused {
                return Ok(None);
            }
            return Err(err);
        }

        // Since the server socket has a backlog of 1, at most one client can `connect` before we
        // invoke `accept`. Since our `connect` has completed, we know the client in the queue must
        // be us, so there's no need to check if the wrong client has connected.
        let raw_connected = WinSock::accept(raw_server, None, None)?;
        let connected = OwnedSocket::from_raw_socket(raw_connected.0 as RawSocket);

        // I'm a little paranoid about the backlog size: if it's treated like a hint rather than
        // an exact queue length, `connect` can succeed despite another connection already being
        // present. So just to make sure, validate that another process hasn't stolen our socket.
        let mut pid = 0u32;
        if WinSock::ioctlsocket(
            raw_connected,
            WinSock::SIO_AF_UNIX_GETPEERPID as i32,
            (&raw mut pid).cast(),
        ) != 0
        {
            return Err(Error::from_raw_os_error(WinSock::WSAGetLastError().0));
        }
        if pid != Threading::GetCurrentProcessId() {
            return Ok(None);
        }

        Ok(Some((TcpStream::from(client), TcpStream::from(connected))))
    }
}

pub(crate) fn serialize_with_handles<T: Object>(value: T) -> Result<Vec<u8>> {
    let broker = HANDLE_BROKER
        .get()
        .expect("broker has not been initialized");

    let mut s = Serializer::new();
    s.serialize(value);

    let copy_handle = |handle: RawHandle| -> Result<usize> {
        let mut remote_handle: HANDLE = Default::default();
        unsafe {
            Foundation::DuplicateHandle(
                Threading::GetCurrentProcess(),
                HANDLE(handle),
                HANDLE(broker.process.as_raw_handle()),
                &mut remote_handle,
                0,
                false,
                Foundation::DUPLICATE_SAME_ACCESS,
            )?;
        }
        Ok(remote_handle.0.addr())
    };

    let remote_handles = s
        .handles
        .into_iter()
        .map(|handle| copy_handle(handle.as_raw_handle()))
        .collect::<Result<Vec<usize>>>()?;

    let remote_sockets = s
        .sockets
        .into_iter()
        .map(|socket| {
            // On modern Windows, sockets are always valid handles. Just make sure to let the normal
            // `OwnedSocket` destructor to be invoked, so that it's closed by `closesocket` rather
            // than `CloseHandle` to correctly free userland resources.
            copy_handle(socket.as_raw_socket() as RawHandle)
        })
        .collect::<Result<Vec<usize>>>()?;

    let mut s1 = Serializer::new();
    s1.serialize(remote_handles);
    s1.serialize(remote_sockets);
    s1.write(&s.data);
    Ok(s1.data)
}

pub(crate) unsafe fn deserialize_with_handles<T: Object>(serialized: Vec<u8>) -> Result<T> {
    let broker = HANDLE_BROKER
        .get()
        .expect("broker has not been initialized");

    let mut d = Deserializer::from(Serializer {
        data: serialized,
        handles: Vec::new(),
        sockets: Vec::new(),
    });

    let steal_handle = |remote_handle: usize| -> Result<OwnedHandle> {
        let mut handle = Default::default();
        unsafe {
            Foundation::DuplicateHandle(
                HANDLE(broker.process.as_raw_handle()),
                HANDLE(core::ptr::without_provenance_mut(remote_handle)),
                Threading::GetCurrentProcess(),
                &mut handle,
                0,
                false,
                Foundation::DUPLICATE_CLOSE_SOURCE | Foundation::DUPLICATE_SAME_ACCESS,
            )
        }?;
        Ok(unsafe { OwnedHandle::from_raw_handle(handle.0) })
    };

    let handles = unsafe { d.deserialize::<Vec<usize>>() }
        .into_iter()
        .map(steal_handle)
        .collect::<Result<Vec<_>>>()?;

    let sockets = unsafe { d.deserialize::<Vec<usize>>() }
        .into_iter()
        .map(|remote_socket| {
            // Modern Winsock2 lazily fetches socket information from the native handle even without
            // going through `WSADuplicateSocketW`/`WSASocket`, see
            // https://purplesyringa.moe/blog/duplicatehandle-works-on-sockets-mostly.
            let socket = steal_handle(remote_socket)?.into_raw_handle() as RawSocket;
            Ok(unsafe { OwnedSocket::from_raw_socket(socket) })
        })
        .collect::<Result<Vec<_>>>()?;

    let data = d.get_rest().to_vec();
    Ok(unsafe {
        Deserializer::from(Serializer {
            data,
            handles,
            sockets,
        })
        .deserialize()
    })
}
