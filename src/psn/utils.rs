use crate::psn::UpdateError;

use std::fmt;
#[cfg(feature = "egui")]
use std::io::{Error, SeekFrom};
#[cfg(feature = "egui")]
use std::path::PathBuf;

use hmac::{Hmac, Mac};
use sha2::Sha256;
#[cfg(feature = "egui")]
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
}

impl fmt::Display for PlatformVariant {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

pub fn get_platform_variant(title_id: &str) -> Option<PlatformVariant> {
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
        PlatformVariant::PS3 => Ok(format!(
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
    Ok(format!("{hash_bytes:x}"))
}

#[cfg(feature = "egui")]
const MERGE_CHUNK_SIZE: usize = 1024 * 1024 * 128;
#[cfg(feature = "egui")]
pub async fn copy_pkg_file(src_path: &PathBuf, target_path: &PathBuf, offset: u64) -> Result<u64, Error> {
    let src_file = OpenOptions::default()
        .create(false)
        .read(true)
        .write(false)
        .open(src_path)
        .await?;

    let mut target_file = OpenOptions::default()
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
