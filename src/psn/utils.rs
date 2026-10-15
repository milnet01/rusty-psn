use crate::psn::UpdateError;

use std::fmt;
use std::io::{Error, SeekFrom};
use std::path::PathBuf;

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use tokio::{
    fs::OpenOptions,
    io::{copy_buf, AsyncSeekExt, BufReader, BufWriter},
};

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PlatformVariant {
    PS3,
    PS4,
    PSVita,
    // Display prints the variant's name, so it is spelt as users see it.
    #[allow(clippy::upper_case_acronyms)]
    PSP,
}

impl fmt::Display for PlatformVariant {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

pub fn get_platform_variant(title_id: &str) -> Option<PlatformVariant> {
    // PSP (RPSN-0075): discs are UCxx and ULxx; PSN games and minis are NP, a region letter, then
    // G, H or Z (NPUG, NPEH, NPUZ), where PS3's own PSN serials have A or B.
    let bytes = title_id.as_bytes();
    let psp_psn = title_id.starts_with("NP") && matches!(bytes.get(3), Some(b'G' | b'H' | b'Z'));
    if title_id.starts_with("UC") || title_id.starts_with("UL") || psp_psn {
        return Some(PlatformVariant::PSP);
    }

    if ["NP", "BL", "BC"].iter().any(|&prefix| title_id.starts_with(prefix)) {
        return Some(PlatformVariant::PS3);
    }

    if title_id.starts_with("CUSA") {
        return Some(PlatformVariant::PS4);
    }

    // PSVita: PCSx (digital), VCxx/VLxx (physical card)
    if ["PCS", "VC", "VL"].iter().any(|&prefix| title_id.starts_with(prefix)) {
        return Some(PlatformVariant::PSVita);
    }

    None
}

pub fn get_update_info_url(title_id: &str, platform_variant: PlatformVariant) -> Result<String, UpdateError> {
    match platform_variant {
        // PSP updates are listed at the same address as PS3's: measured 2026-10-15 with NPUG80318.
        PlatformVariant::PS3 | PlatformVariant::PSP => Ok(format!(
            "https://a0.ww.np.dl.playstation.net/tpl/np/{0}/{0}-ver.xml",
            title_id
        )),
        PlatformVariant::PS4 => {
            // This HMAC-SHA256 key is publicly known and embedded in every PS4 console's firmware.
            // It is used by Sony's public update API to generate per-title URL paths for update metadata.
            // The key has been documented by the homebrew community since at least 2018 and is used by
            // multiple open-source tools including PKGj, NPS Browser, and VitaDB.
            // Hardcoding is acceptable because this is not a secret — it is a public API parameter.
            let hash = hmac_url_hash("AD62E37F905E06BC19593142281C112CEC0E7EC3E97EFDCAEFCDBAAFA6378D84", title_id)?;
            Ok(format!(
                "https://gs-sec.ww.np.dl.playstation.net/plo/np/{0}/{1}/{0}-ver.xml",
                title_id, hash
            ))
        }
        PlatformVariant::PSVita => {
            // PSVita uses the same HMAC scheme as PS4 but with a different key and URL path (/pl/ vs /plo/).
            // Key provenance: discovered by developer Proxima, published via VTU_HMAC tool, widely used
            // by PKGj, NPS Browser, and VitaDB. Publicly known since ~2018.
            let hash = hmac_url_hash("E5E278AA1EE34082A088279C83F9BBC806821C52F2AB5D2B4ABD995450355114", title_id)?;
            Ok(format!(
                "https://gs-sec.ww.np.dl.playstation.net/pl/np/{0}/{1}/{0}-ver.xml",
                title_id, hash
            ))
        }
    }
}

/// Compute the HMAC-SHA256 hash for a Sony update URL.
/// Input: hex-encoded key, title_id. Output: lowercase hex hash string.
fn hmac_url_hash(hex_key: &str, title_id: &str) -> Result<String, UpdateError> {
    let key = hex::decode(hex_key).map_err(|_| UpdateError::InvalidSerial)?;
    let msg = format!("np_{title_id}");
    let mut hasher = HmacSha256::new_from_slice(&key).map_err(|_| UpdateError::InvalidSerial)?;
    hasher.update(msg.as_ref());
    let hash_bytes = hasher.finalize().into_bytes();
    Ok(hex::encode(hash_bytes))
}

#[derive(Debug)]
pub enum CappedReadError {
    Reqwest(reqwest::Error),
    /// The reply passed the cap. Holds its stated length, or the bytes read before reading stopped.
    TooLarge(u64),
}

/// Read a reply's body as text, refusing it once it passes `cap` bytes.
/// Content-Length is checked first, but a chunked reply carries none, so the body is also
/// counted as it arrives and reading stops past the cap (RPSN-0151).
/// Decodes as `Response::text` does without reqwest's `charset` feature: UTF-8, lossy.
pub async fn read_capped_text(mut response: reqwest::Response, cap: u64) -> Result<String, CappedReadError> {
    if let Some(len) = response.content_length() {
        if len > cap {
            return Err(CappedReadError::TooLarge(len));
        }
    }

    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(CappedReadError::Reqwest)? {
        let read = body.len() as u64 + chunk.len() as u64;
        if read > cap {
            return Err(CappedReadError::TooLarge(read));
        }
        body.extend_from_slice(&chunk);
    }

    Ok(String::from_utf8_lossy(&body).into_owned())
}

const MERGE_CHUNK_SIZE: usize = 1024 * 1024 * 128;
pub async fn copy_pkg_file(src_path: &PathBuf, target_path: &PathBuf, offset: u64) -> Result<u64, Error> {
    let src_file = OpenOptions::default()
        .create(false)
        .read(true)
        .write(false)
        .open(src_path)
        .await?;

    let mut target_file = crate::utils::download_file_options()
        .create(true)
        .read(false)
        .write(true)
        .open(target_path)
        .await?;

    if offset > 0 {
        target_file.seek(SeekFrom::Start(offset)).await?;
    }

    let mut writer = BufWriter::with_capacity(MERGE_CHUNK_SIZE, target_file);
    let mut reader = BufReader::with_capacity(MERGE_CHUNK_SIZE, src_file);
    let read_bytes = copy_buf(&mut reader, &mut writer).await?;
    Ok(read_bytes)
}

#[cfg(test)]
mod tests {
    use super::{read_capped_text, CappedReadError};
    use std::io::{Read, Write};
    use std::thread::JoinHandle;

    const CHUNK: usize = 1024;

    /// Serve one chunked reply with no Content-Length on a local port, `total` bytes long.
    /// The thread returns how many bytes it wrote before the client hung up.
    fn serve_chunked(total: usize) -> (String, JoinHandle<usize>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = stream.read(&mut [0u8; 1024]);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n");
            let chunk = format!("{CHUNK:x}\r\n{}\r\n", "a".repeat(CHUNK));
            let mut sent = 0;
            while sent < total && stream.write_all(chunk.as_bytes()).is_ok() {
                sent += CHUNK;
            }
            let _ = stream.write_all(b"0\r\n\r\n");
            sent
        });
        (url, server)
    }

    #[tokio::test]
    async fn a_chunked_reply_stops_being_read_past_the_cap() {
        // A reply with no Content-Length used to be read whole before its size was checked (RPSN-0151).
        let total = 64 * 1024 * 1024;
        let (url, server) = serve_chunked(total);
        let client = reqwest::Client::new();
        let response = client.get(&url).send().await.unwrap();

        let result = read_capped_text(response, 4 * CHUNK as u64).await;
        drop(client);

        assert!(matches!(result, Err(CappedReadError::TooLarge(_))));
        // Joined off the runtime, which must stay free to close the dropped connection.
        let sent = tokio::task::spawn_blocking(move || server.join().unwrap()).await.unwrap();
        assert!(sent < total, "the whole reply was read");
    }

    #[tokio::test]
    async fn a_chunked_reply_within_the_cap_is_returned_whole() {
        let (url, server) = serve_chunked(3 * CHUNK);
        let response = reqwest::Client::new().get(&url).send().await.unwrap();

        let text = read_capped_text(response, 3 * CHUNK as u64).await.unwrap();

        assert_eq!(text, "a".repeat(3 * CHUNK));
        tokio::task::spawn_blocking(move || server.join().unwrap()).await.unwrap();
    }
}
