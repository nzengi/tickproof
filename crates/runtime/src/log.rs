//! Input log on disk. Genesis state plus this log is the whole game:
//! anyone can replay it and land on the same state roots, which is what
//! a dispute ultimately falls back on.
//!
//! Format: 4-byte magic, then one u32 LE length prefix per entry, read
//! to EOF. No entry count up front - the file stays append-friendly.

use std::io::{self, Read, Write};

const MAGIC: &[u8; 4] = b"TPL1";

// A tick's inputs are tiny; anything near this is a corrupt file, not a
// real entry. Keeps a bad length prefix from turning into a 4 GiB alloc.
const MAX_ENTRY: u32 = 1 << 20;

pub fn write_log<W: Write>(mut w: W, entries: &[Vec<u8>]) -> io::Result<()> {
    w.write_all(MAGIC)?;
    for entry in entries {
        let len = u32::try_from(entry.len())
            .ok()
            .filter(|&l| l <= MAX_ENTRY)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "entry too long"))?;
        w.write_all(&len.to_le_bytes())?;
        w.write_all(entry)?;
    }
    Ok(())
}

pub fn read_log<R: Read>(mut r: R) -> io::Result<Vec<Vec<u8>>> {
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not a tickproof input log",
        ));
    }

    let mut entries = Vec::new();
    let mut len_buf = [0u8; 4];
    loop {
        match r.read_exact(&mut len_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e),
        }
        let len = u32::from_le_bytes(len_buf);
        if len > MAX_ENTRY {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "entry length out of range",
            ));
        }
        let mut entry = vec![0u8; len as usize];
        r.read_exact(&mut entry)?;
        entries.push(entry);
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn roundtrip() {
        let entries = vec![vec![], vec![1, 2, 3], vec![0u8; 500]];
        let mut buf = Vec::new();
        write_log(&mut buf, &entries).unwrap();
        assert_eq!(read_log(Cursor::new(buf)).unwrap(), entries);
    }

    #[test]
    fn empty_log() {
        let mut buf = Vec::new();
        write_log(&mut buf, &[]).unwrap();
        assert_eq!(buf, b"TPL1");
        assert!(read_log(Cursor::new(buf)).unwrap().is_empty());
    }

    #[test]
    fn bad_magic_rejected() {
        assert!(read_log(Cursor::new(b"NOPE".to_vec())).is_err());
    }

    #[test]
    fn truncated_entry_rejected() {
        let mut buf = Vec::new();
        write_log(&mut buf, &[vec![7u8; 100]]).unwrap();
        buf.truncate(buf.len() - 1);
        assert!(read_log(Cursor::new(buf)).is_err());
    }

    #[test]
    fn huge_length_prefix_rejected() {
        let mut buf = b"TPL1".to_vec();
        buf.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(read_log(Cursor::new(buf)).is_err());
    }
}
