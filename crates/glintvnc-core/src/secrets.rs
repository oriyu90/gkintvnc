//! Keychain is the exclusive storage for passwords. Never print or serialize a secret.
use std::io;
const SERVICE: &str = "dev.glintvnc.viewer.v1";

pub fn set_password(id: &str, password: &str) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let e = keyring::Entry::new(SERVICE, id).map_err(io::Error::other)?;
        e.set_password(password).map_err(io::Error::other)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (id, password);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Keychain only supported on macOS",
        ))
    }
}
pub fn get_password(id: &str) -> io::Result<Option<String>> {
    #[cfg(target_os = "macos")]
    {
        let e = keyring::Entry::new(SERVICE, id).map_err(io::Error::other)?;
        match e.get_password() {
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(io::Error::other(e)),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = id;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Keychain only supported on macOS",
        ))
    }
}
pub fn delete_password(id: &str) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let e = keyring::Entry::new(SERVICE, id).map_err(io::Error::other)?;
        match e.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(io::Error::other(e)),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = id;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Keychain only supported on macOS",
        ))
    }
}
