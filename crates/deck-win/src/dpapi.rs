//! DPAPI: mit dem Windows-Konto verschlüsselte Daten entschlüsseln (aus
//! `claude/usage_token.py`, nur `_dpapi_unprotect`).
//!
//! Gebraucht für das Token von Claude Desktop: dessen `Local State` hält den
//! AES-Schlüssel DPAPI-verschlüsselt (`os_crypt.encrypted_key`, Base64, mit fünf Bytes
//! „DPAPI“ davor). Den Rest der Kette (Base64, Präfix abschneiden, AES-256-GCM, Token
//! auswählen) macht die App.

use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptUnprotectData};

use crate::types::{Error, Result};

/// `CryptUnprotectData` für das aktuelle Windows-Konto, ohne Zusatz-Entropie und ohne
/// Rückfrage an den Nutzer.
pub fn unprotect(data: &[u8]) -> Result<Vec<u8>> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: input zeigt auf `data` (wird nur gelesen), output wird von Windows per
    // LocalAlloc gefüllt und hier wieder freigegeben.
    unsafe {
        CryptUnprotectData(&input, None, None, None, None, 0, &mut output)
            .map_err(|e| Error::win("CryptUnprotectData", e))?;
        Ok(take(output))
    }
}

/// Bytes aus einem von Windows angelegten Blob kopieren und den Blob freigeben.
unsafe fn take(blob: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    if blob.pbData.is_null() {
        return Vec::new();
    }
    // SAFETY: Windows garantiert cbData lesbare Bytes ab pbData; danach LocalFree.
    unsafe {
        let out = std::slice::from_raw_parts(blob.pbData, blob.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(blob.pbData.cast())));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Security::Cryptography::CryptProtectData;
    use windows::core::PCWSTR;

    fn protect(data: &[u8]) -> Vec<u8> {
        let input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        // SAFETY: wie in unprotect.
        unsafe {
            CryptProtectData(&input, PCWSTR::null(), None, None, None, 0, &mut output).unwrap();
            take(output)
        }
    }

    #[test]
    fn what_this_account_protected_it_can_unprotect() {
        // Harmlos: verschlüsselt und entschlüsselt nur im Speicher, mit dem eigenen Konto.
        let secret = b"geheimer Schluessel \x00\x01\xff";
        let blob = protect(secret);
        assert_ne!(blob.as_slice(), secret.as_slice());
        assert_eq!(unprotect(&blob).unwrap(), secret);
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(matches!(
            unprotect(b"kein DPAPI-Blob"),
            Err(Error::Win32 { .. })
        ));
    }
}
