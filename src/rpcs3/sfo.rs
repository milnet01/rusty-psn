use std::path::Path;

use log::warn;

/// Metadata extracted from a PARAM.SFO file.
#[allow(dead_code)]
pub struct SfoParams {
    pub title: String,
    pub title_id: String,
    pub app_ver: Option<String>,
    pub version: String,
    pub category: String,
}

/// Maximum file size for a PARAM.SFO (1 MB — real files are under 1 KB).
const MAX_SFO_SIZE: u64 = 1_048_576;

/// PSF magic bytes: `\0PSF`
const PSF_MAGIC: [u8; 4] = [0x00, 0x50, 0x53, 0x46];

/// UTF-8 string data format in SFO index entries.
const DATA_FMT_UTF8: u16 = 0x0204;

fn read_u16_le(data: &[u8], offset: usize) -> Option<u16> {
    let bytes = data.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32_le(data: &[u8], offset: usize) -> Option<u32> {
    let bytes = data.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Read a null-terminated UTF-8 string starting at `offset`, up to `max_len` bytes.
fn read_string(data: &[u8], offset: usize, max_len: usize) -> Option<String> {
    let slice = data.get(offset..)?;
    let end = slice
        .iter()
        .take(max_len)
        .position(|&b| b == 0)
        .unwrap_or(max_len.min(slice.len()));
    std::str::from_utf8(slice.get(..end)?).ok().map(|s| s.to_string())
}

/// Parse a PARAM.SFO binary file and extract key metadata fields.
pub fn parse_sfo(path: &Path) -> Result<SfoParams, String> {
    let metadata = std::fs::metadata(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if metadata.len() > MAX_SFO_SIZE {
        return Err(format!("SFO file too large: {} bytes", metadata.len()));
    }

    let data = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;

    if data.len() < 20 {
        return Err("SFO file too small (< 20 bytes)".to_string());
    }

    // Validate magic bytes.
    if data[..4] != PSF_MAGIC {
        return Err(format!(
            "invalid SFO magic: expected 00505346, got {:02x}{:02x}{:02x}{:02x}",
            data[0], data[1], data[2], data[3]
        ));
    }

    let key_table_start = read_u32_le(&data, 8).ok_or("truncated header")? as usize;
    let data_table_start = read_u32_le(&data, 12).ok_or("truncated header")? as usize;
    let num_entries = read_u32_le(&data, 16).ok_or("truncated header")? as usize;

    if key_table_start > data.len() || data_table_start > data.len() {
        return Err("SFO table offsets exceed file size".to_string());
    }

    let mut title = None;
    let mut title_id = None;
    let mut app_ver = None;
    let mut version = None;
    let mut category = None;

    // Index table starts at offset 0x14 (20), each entry is 16 bytes.
    for i in 0..num_entries {
        let entry_offset = 20 + i * 16;
        if entry_offset + 16 > data.len() {
            warn!("SFO index entry {i} exceeds file size");
            break;
        }

        let key_offset = read_u16_le(&data, entry_offset).ok_or("truncated index entry")? as usize;
        let data_fmt = read_u16_le(&data, entry_offset + 2).ok_or("truncated index entry")?;
        let data_len = read_u32_le(&data, entry_offset + 4).ok_or("truncated index entry")? as usize;
        let data_offset = read_u32_le(&data, entry_offset + 12).ok_or("truncated index entry")? as usize;

        // Read the key name from the key table.
        let abs_key_offset = key_table_start + key_offset;
        let key = match read_string(&data, abs_key_offset, 64) {
            Some(k) => k,
            None => continue,
        };

        // Only extract UTF-8 string values for the fields we care about.
        if data_fmt != DATA_FMT_UTF8 {
            continue;
        }

        let abs_data_offset = data_table_start + data_offset;
        let value = match read_string(&data, abs_data_offset, data_len) {
            Some(v) => v,
            None => continue,
        };

        // Values are printed to the terminal, so control characters are dropped.
        let value = crate::security::strip_control(&value);
        match key.as_str() {
            "TITLE" => title = Some(value),
            "TITLE_ID" => title_id = Some(value),
            "APP_VER" => app_ver = Some(value),
            "VERSION" => version = Some(value),
            "CATEGORY" => category = Some(value),
            _ => {}
        }
    }

    Ok(SfoParams {
        title: title.unwrap_or_default(),
        title_id: title_id.unwrap_or_default(),
        app_ver,
        version: version.unwrap_or_default(),
        category: category.unwrap_or_default(),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // Minimal valid SFO with TITLE="Test Game", TITLE_ID="BCUS99999", APP_VER="01.05", VERSION="01.00"
    pub(crate) fn make_test_sfo() -> Vec<u8> {
        let mut data = Vec::new();

        // Keys: "APP_VER\0TITLE\0TITLE_ID\0VERSION\0"
        let keys = b"APP_VER\0TITLE\0TITLE_ID\0VERSION\0";
        // Values: "01.05\0" + padding, "Test Game\0" + padding, "BCUS99999\0" + padding, "01.00\0" + padding
        let val_app_ver = b"01.05\0\0\0"; // 8 bytes (data_len=6, max=8)
        let val_title = b"Test Game\0\0\0\0\0\0\0"; // 16 bytes (data_len=10, max=16)
        let val_title_id = b"BCUS99999\0\0\0\0\0\0\0"; // 16 bytes (data_len=10, max=16)
        let val_version = b"01.00\0\0\0"; // 8 bytes (data_len=6, max=8)

        let num_entries: u32 = 4;
        let index_size = num_entries as usize * 16;
        let key_table_start = (20 + index_size) as u32;
        let data_table_start = key_table_start + keys.len() as u32;

        // Header
        data.extend_from_slice(&PSF_MAGIC); // magic
        data.extend_from_slice(&0x00000101u32.to_le_bytes()); // version 1.1
        data.extend_from_slice(&key_table_start.to_le_bytes());
        data.extend_from_slice(&data_table_start.to_le_bytes());
        data.extend_from_slice(&num_entries.to_le_bytes());

        // Index entries: key_offset(u16), data_fmt(u16), data_len(u32), data_max_len(u32), data_offset(u32)
        let mut data_offset: u32 = 0;

        // Entry 0: APP_VER (key offset 0)
        data.extend_from_slice(&0u16.to_le_bytes()); // key_offset
        data.extend_from_slice(&DATA_FMT_UTF8.to_le_bytes());
        data.extend_from_slice(&6u32.to_le_bytes()); // data_len
        data.extend_from_slice(&8u32.to_le_bytes()); // data_max_len
        data.extend_from_slice(&data_offset.to_le_bytes());
        data_offset += 8;

        // Entry 1: TITLE (key offset 8)
        data.extend_from_slice(&8u16.to_le_bytes());
        data.extend_from_slice(&DATA_FMT_UTF8.to_le_bytes());
        data.extend_from_slice(&10u32.to_le_bytes());
        data.extend_from_slice(&16u32.to_le_bytes());
        data.extend_from_slice(&data_offset.to_le_bytes());
        data_offset += 16;

        // Entry 2: TITLE_ID (key offset 14)
        data.extend_from_slice(&14u16.to_le_bytes());
        data.extend_from_slice(&DATA_FMT_UTF8.to_le_bytes());
        data.extend_from_slice(&10u32.to_le_bytes());
        data.extend_from_slice(&16u32.to_le_bytes());
        data.extend_from_slice(&data_offset.to_le_bytes());
        data_offset += 16;

        // Entry 3: VERSION (key offset 23)
        data.extend_from_slice(&23u16.to_le_bytes());
        data.extend_from_slice(&DATA_FMT_UTF8.to_le_bytes());
        data.extend_from_slice(&6u32.to_le_bytes());
        data.extend_from_slice(&8u32.to_le_bytes());
        data.extend_from_slice(&data_offset.to_le_bytes());

        // Key table
        data.extend_from_slice(keys);

        // Data table
        data.extend_from_slice(val_app_ver);
        data.extend_from_slice(val_title);
        data.extend_from_slice(val_title_id);
        data.extend_from_slice(val_version);

        data
    }

    #[test]
    fn parse_valid_sfo() {
        let data = make_test_sfo();
        let mut f = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut f, &data).unwrap();

        let params = parse_sfo(f.path()).unwrap();
        assert_eq!(params.title, "Test Game");
        assert_eq!(params.title_id, "BCUS99999");
        assert_eq!(params.app_ver, Some("01.05".to_string()));
        assert_eq!(params.version, "01.00");
    }

    #[test]
    fn parse_wrong_magic() {
        let mut data = make_test_sfo();
        data[0] = 0xFF;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut f, &data).unwrap();

        assert!(parse_sfo(f.path()).is_err());
    }

    #[test]
    fn parse_truncated_file() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut f, &[0x00, 0x50, 0x53, 0x46]).unwrap();

        assert!(parse_sfo(f.path()).is_err());
    }

    #[test]
    fn parse_nonexistent_file() {
        assert!(parse_sfo(Path::new("/nonexistent/PARAM.SFO")).is_err());
    }
}
