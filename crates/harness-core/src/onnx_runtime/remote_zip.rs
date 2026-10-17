//! Reads single files out of a remote ZIP archive with HTTP range requests.
//!
//! ONNX Runtime ships as big archives of which Harness needs two or three files: the Windows
//! CPU release is 78 MB (a 408 MB `.pdb` inside), the DirectML redistributable NuGet package
//! 202 MB for one 18 MB DLL. A ZIP keeps its table of contents at the end, so the files can be
//! fetched on their own: the tail (end of central directory + central directory), then each
//! wanted entry's local header and compressed bytes. The caller verifies the extracted bytes
//! against a pinned SHA-256; the ZIP's own CRC-32 is checked here as well.

use reqwest::header::{CONTENT_RANGE, RANGE};

const EOCD_SIG: u32 = 0x0605_4b50;
const CDIR_SIG: u32 = 0x0201_4b50;
const LOCAL_SIG: u32 = 0x0403_4b50;
/// Bytes read from the end of the archive: the central directory of the archives here is a
/// few KiB, the end record 22 bytes plus a comment.
const TAIL: u64 = 64 * 1024;

/// One entry of the central directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub method: u16,
    pub crc32: u32,
    pub compressed: u64,
    pub size: u64,
    pub local_offset: u64,
}

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// Parses the central directory records in `cd`.
pub fn parse_central_directory(cd: &[u8]) -> Result<Vec<Entry>, String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 46 <= cd.len() {
        if u32_at(cd, i) != Some(CDIR_SIG) {
            return Err("zip: bad central directory record".into());
        }
        let field = |off: usize| u32_at(cd, i + off).ok_or("zip: truncated record");
        let nlen = u16_at(cd, i + 28).ok_or("zip: truncated record")? as usize;
        let elen = u16_at(cd, i + 30).ok_or("zip: truncated record")? as usize;
        let clen = u16_at(cd, i + 32).ok_or("zip: truncated record")? as usize;
        let name = cd
            .get(i + 46..i + 46 + nlen)
            .ok_or("zip: truncated name")?
            .to_vec();
        let name = String::from_utf8(name).map_err(|_| "zip: invalid UTF-8 entry name")?;
        // Fail closed on symlink mode bits, regardless of the claimed producer platform.
        let mode = Some(field(38)? >> 16);
        crate::zip_safety::validate_entry(&name, mode)?;
        out.push(Entry {
            name,
            method: u16_at(cd, i + 10).ok_or("zip: truncated record")?,
            crc32: field(16)?,
            compressed: u64::from(field(20)?),
            size: u64::from(field(24)?),
            local_offset: u64::from(field(42)?),
        });
        i += 46 + nlen + elen + clen;
    }
    Ok(out)
}

/// Locates the central directory from the archive's tail: `(offset, size)`.
pub fn find_central_directory(tail: &[u8]) -> Result<(u64, u64), String> {
    let at = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|&i| u32_at(tail, i) == Some(EOCD_SIG))
        .ok_or("zip: no end-of-central-directory record")?;
    let size = u32_at(tail, at + 12).ok_or("zip: truncated end record")?;
    let offset = u32_at(tail, at + 16).ok_or("zip: truncated end record")?;
    if size == u32::MAX || offset == u32::MAX {
        return Err("zip: ZIP64 archives are not read with range requests".into());
    }
    Ok((u64::from(offset), u64::from(size)))
}

/// Inflates (or copies) the stored bytes of `entry` and checks size and CRC-32.
pub fn decode_entry(entry: &Entry, data: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let bytes = match entry.method {
        0 => data.to_vec(),
        8 => {
            let mut out = Vec::with_capacity(entry.size as usize);
            flate2::read::DeflateDecoder::new(data)
                .read_to_end(&mut out)
                .map_err(|e| format!("zip: cannot inflate {}: {}", entry.name, e))?;
            out
        }
        m => return Err(format!("zip: {} uses compression method {}", entry.name, m)),
    };
    if bytes.len() as u64 != entry.size {
        return Err(format!(
            "zip: {} is {} bytes, expected {}",
            entry.name,
            bytes.len(),
            entry.size
        ));
    }
    let mut crc = flate2::Crc::new();
    crc.update(&bytes);
    if crc.sum() != entry.crc32 {
        return Err(format!("zip: CRC mismatch in {}", entry.name));
    }
    Ok(bytes)
}

async fn range(
    client: &reqwest::Client,
    url: &str,
    spec: String,
) -> Result<(Vec<u8>, Option<u64>), String> {
    let res = client
        .get(url)
        .header(RANGE, spec)
        .send()
        .await
        .map_err(|e| crate::http_error::transport(&e, url))?;
    if res.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(format!(
            "{} does not support range requests (HTTP {})",
            url,
            res.status().as_u16()
        ));
    }
    let total = res
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit('/').next())
        .and_then(|v| v.parse::<u64>().ok());
    let bytes = res
        .bytes()
        .await
        .map_err(|e| crate::http_error::transport(&e, url))?;
    Ok((bytes.to_vec(), total))
}

/// Fetches `wanted` (entry names) from the ZIP at `url`. `on_bytes` receives the number of
/// bytes fetched so far. Returns `(name, content)` in the order of `wanted`.
pub async fn fetch_entries(
    client: &reqwest::Client,
    url: &str,
    wanted: &[&str],
    on_bytes: &(dyn Fn(u64) + Send + Sync),
) -> Result<Vec<(String, Vec<u8>)>, String> {
    // The size first: GitHub's release CDN answers `bytes=-N` (a suffix) with 501, explicit
    // ranges work everywhere.
    let (_, total) = range(client, url, "bytes=0-0".to_string()).await?;
    let total = total.ok_or("zip: the server did not report the archive size")?;
    let tail_start = total.saturating_sub(TAIL);
    let (tail, _) = range(client, url, format!("bytes={}-{}", tail_start, total - 1)).await?;
    let mut fetched = tail.len() as u64;
    on_bytes(fetched);
    let (cd_offset, cd_size) = find_central_directory(&tail)?;
    let cd = if cd_offset >= tail_start {
        let from = (cd_offset - tail_start) as usize;
        tail.get(from..from + cd_size as usize)
            .ok_or("zip: central directory outside the tail")?
            .to_vec()
    } else {
        let (cd, _) = range(
            client,
            url,
            format!("bytes={}-{}", cd_offset, cd_offset + cd_size - 1),
        )
        .await?;
        fetched += cd.len() as u64;
        on_bytes(fetched);
        cd
    };
    let entries = parse_central_directory(&cd)?;

    let mut out = Vec::new();
    for name in wanted {
        let entry = entries
            .iter()
            .find(|e| e.name == *name)
            .ok_or_else(|| format!("zip: {} is not in the archive", name))?;
        let (head, _) = range(
            client,
            url,
            format!("bytes={}-{}", entry.local_offset, entry.local_offset + 29),
        )
        .await?;
        if u32_at(&head, 0) != Some(LOCAL_SIG) {
            return Err(format!("zip: bad local header for {}", name));
        }
        let nlen = u64::from(u16_at(&head, 26).ok_or("zip: truncated local header")?);
        let elen = u64::from(u16_at(&head, 28).ok_or("zip: truncated local header")?);
        let start = entry.local_offset + 30 + nlen + elen;
        let data = if entry.compressed == 0 {
            Vec::new()
        } else {
            range(
                client,
                url,
                format!("bytes={}-{}", start, start + entry.compressed - 1),
            )
            .await?
            .0
        };
        fetched += data.len() as u64;
        on_bytes(fetched);
        out.push((name.to_string(), decode_entry(entry, &data)?));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A real ZIP written by the `zip` crate: one stored, one deflated file.
    fn archive() -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let stored = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            let deflated = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            w.start_file("lib/a.txt", stored).unwrap();
            w.write_all(b"hello").unwrap();
            w.start_file("lib/b.bin", deflated).unwrap();
            w.write_all(&vec![7u8; 5000]).unwrap();
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn central_directory_and_entries_are_read_from_the_tail() {
        let zip = archive();
        let (offset, size) = find_central_directory(&zip).unwrap();
        let cd = &zip[offset as usize..(offset + size) as usize];
        let entries = parse_central_directory(cd).unwrap();
        assert_eq!(entries.len(), 2);
        let a = entries.iter().find(|e| e.name == "lib/a.txt").unwrap();
        let b = entries.iter().find(|e| e.name == "lib/b.bin").unwrap();
        assert_eq!((a.method, a.size), (0, 5));
        assert_eq!((b.method, b.size), (8, 5000));
        assert!(b.compressed < 100);

        // The bytes a range request for the entry would return.
        let data_of = |e: &Entry| {
            let lo = e.local_offset as usize;
            let nlen = u16_at(&zip, lo + 26).unwrap() as usize;
            let elen = u16_at(&zip, lo + 28).unwrap() as usize;
            let start = lo + 30 + nlen + elen;
            zip[start..start + e.compressed as usize].to_vec()
        };
        assert_eq!(decode_entry(a, &data_of(a)).unwrap(), b"hello");
        assert_eq!(decode_entry(b, &data_of(b)).unwrap(), vec![7u8; 5000]);
    }

    #[test]
    fn corrupt_data_fails_the_crc() {
        let zip = archive();
        let (offset, size) = find_central_directory(&zip).unwrap();
        let entries =
            parse_central_directory(&zip[offset as usize..(offset + size) as usize]).unwrap();
        let a = entries.iter().find(|e| e.name == "lib/a.txt").unwrap();
        let err = decode_entry(a, b"hellp").unwrap_err();
        assert!(err.contains("CRC"), "{err}");
        assert!(decode_entry(a, b"hell").unwrap_err().contains("expected 5"));
    }

    #[test]
    fn garbage_is_not_a_zip() {
        assert!(find_central_directory(b"not a zip at all, no end record here....").is_err());
        assert!(parse_central_directory(&[0u8; 60]).is_err());
    }
}
