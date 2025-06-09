//! Doom‑format WAD loader.
//!
//! ### Supported files
//! * **IWAD** – main game data shipped by id Software.

use std::collections::HashMap;

use byteorder::{LittleEndian as LE, ReadBytesExt};
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
};
use thiserror::Error;

/// Size (in bytes) of one directory entry.
const DIR_ENTRY_SIZE: usize = 16;

/// Metadata for a single lump (asset) inside the WAD.
#[derive(Clone, Debug)]
pub struct LumpInfo {
    /// Eight‑byte ASCII name, padded with NULs.
    pub name: [u8; 8],
    /// Offset to lump data from the beginning of the file.
    pub offset: u32,
    /// Size of the lump in bytes.
    pub size: u32,
}

/// Entire WAD resident in memory.
#[derive(Debug)]
pub struct Wad {
    /// Directory entries in the exact order they appear in the file.
    pub lumps: Vec<LumpInfo>,
    /// Backing buffer containing the raw file contents.
    bytes: Vec<u8>,
    /// fast name → index lookup
    by_name: HashMap<String, usize>,
}

/// Errors that can be encountered while opening/parsing a WAD.
#[derive(Error, Debug)]
pub enum WadError {
    /// Underlying I/O failure – propagated unchanged.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Header magic wasn’t `IWAD`.
    #[error("not a IWAD file")]
    BadMagic,

    /// Directory claims to extend past end‑of‑file.
    #[error("corrupt WAD: directory extends beyond end of file")]
    DirectoryOutOfBounds,
}

impl Wad {
    // ---------------------------------------------------------------------
    // Loading
    // ---------------------------------------------------------------------

    /// Load a WAD from disk into memory.
    ///
    /// The entire file is read into a `Vec<u8>` so subsequent lump requests
    /// are just slice operations.  Even on old hardware a 25 MiB IWAD loads in
    /// a few milliseconds.
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, WadError> {
        let mut file = File::open(path)?;

        /*----------- 1. read and validate header ------------------------*/
        let mut id: [u8; 4] = [0; 4];
        file.read_exact(&mut id)?;
        if &id != b"IWAD" {
            return Err(WadError::BadMagic);
        }

        let num_lumps = file.read_u32::<LE>()?;
        let dir_offset = file.read_u32::<LE>()?;

        /*----------- 2. read full file into RAM -------------------------*/
        let mut bytes = Vec::new();
        file.seek(SeekFrom::Start(0))?;
        file.read_to_end(&mut bytes)?;

        /*----------- 3. sanity‑check directory bounds -------------------*/
        let dir_end = dir_offset as usize + num_lumps as usize * DIR_ENTRY_SIZE;
        if dir_end > bytes.len() {
            return Err(WadError::DirectoryOutOfBounds);
        }

        /*----------- 4. parse directory entries -------------------------*/
        let mut lumps = Vec::with_capacity(num_lumps as usize);
        let mut cursor = &bytes[dir_offset as usize..dir_end];

        for _ in 0..num_lumps {
            let offset = cursor.read_u32::<LE>()?;
            let size = cursor.read_u32::<LE>()?;
            let mut name = [0u8; 8];
            cursor.read_exact(&mut name)?;
            lumps.push(LumpInfo { name, offset, size });
        }

        for l in &lumps {
            let end = l.offset as usize + l.size as usize;
            if end > bytes.len() {
                return Err(WadError::DirectoryOutOfBounds);
            }
        }

        /*----------- 5. build reverse index -------------------------*/
        let mut by_name = HashMap::with_capacity(lumps.len());
        // scan *backwards* so later lumps override earlier ones
        for (i, l) in lumps.iter().enumerate().rev() {
            // Self::lump_name ‹&[u8;8] → &str› already trims NULs
            by_name
                .entry(Self::lump_name(&l.name).to_owned())
                .or_insert(i);
        }

        Ok(Self {
            lumps,
            bytes,
            by_name,
        })
    }

    // ---------------------------------------------------------------------
    // Convenience helpers
    // ---------------------------------------------------------------------

    /// Convert an eight‑byte, NUL‑padded lump name into a printable string.
    pub fn lump_name(raw: &[u8; 8]) -> &str {
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        std::str::from_utf8(&raw[..end]).unwrap_or("�")
    }

    /// Borrow the raw bytes for lump `idx` without copying.
    pub fn lump_bytes(&self, idx: usize) -> &[u8] {
        assert!(idx < self.lumps.len(), "lump index out of bounds");
        let l = &self.lumps[idx];
        &self.bytes[l.offset as usize..(l.offset + l.size) as usize]
    }

    /// Locate a lump by name (case‑sensitive).  Returns its index in the
    /// directory or `None` if missing.
    pub fn find_lump(&self, name: &str) -> Option<usize> {
        self.by_name.get(name).copied()
    }
}

// ==========================================================================
// Unit tests – run with `cargo test -p wad`
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    /// Locate `<repo>/bin/doom1.wad` so both the engine and the tests share it.
    fn doom_wad() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")) // → <repo>/wad
            .join("assets")
            .join("doom1.wad")
    }

    /*------------------------------------------------------------------*/
    /* 1. Header sanity                                                 */
    /*------------------------------------------------------------------*/
    #[test]
    fn opens_and_reads_header() {
        let wad = Wad::from_file(doom_wad()).expect("cannot open doom1.wad");
        assert!(
            wad.lumps.len() > 100,
            "suspiciously few lumps: {}",
            wad.lumps.len()
        );
    }

    /*------------------------------------------------------------------*/
    /* 2. Essential lumps                                               */
    /*------------------------------------------------------------------*/
    #[test]
    fn essential_lumps_exist() {
        let wad = Wad::from_file(doom_wad()).unwrap();
        for needed in ["PLAYPAL", "COLORMAP", "TITLEPIC"] {
            assert!(
                wad.lumps.iter().any(|l| Wad::lump_name(&l.name) == needed),
                "required lump {needed} missing"
            );
        }
    }

    /*------------------------------------------------------------------*/
    /* 3. TITLEPIC size sanity                                          */
    /*------------------------------------------------------------------*/
    #[test]
    fn titlepic_size_sane() {
        let wad = Wad::from_file(doom_wad()).unwrap();
        let title = wad
            .lumps
            .iter()
            .find(|l| Wad::lump_name(&l.name) == "TITLEPIC")
            .expect("TITLEPIC lump missing");
        assert!(
            (64_000..200_000).contains(&title.size),
            "weird TITLEPIC size: {} bytes",
            title.size
        );
    }

    /*------------------------------------------------------------------*/
    /* 4. Byte‑slice length matches directory                           */
    /*------------------------------------------------------------------*/
    #[test]
    fn lump_slice_len_matches_directory() {
        let wad = Wad::from_file(doom_wad()).unwrap();
        for (i, l) in wad.lumps.iter().enumerate() {
            assert_eq!(
                wad.lump_bytes(i).len() as u32,
                l.size,
                "size mismatch in lump {}",
                Wad::lump_name(&l.name)
            );
        }
    }

    /*------------------------------------------------------------------*/
    /* 5. Bad‑magic guard                                               */
    /*------------------------------------------------------------------*/
    #[test]
    fn rejects_garbage_file() {
        let bogus = doom_wad().with_extension("tmp");
        fs::write(&bogus, b"NOTWAD_____" /* 11 bytes */).unwrap();
        let err = Wad::from_file(&bogus).unwrap_err();
        fs::remove_file(&bogus).unwrap();
        assert!(matches!(err, WadError::BadMagic { .. }));
    }

    /*------------------------------------------------------------------*/
    /* 6. Directory-bounds guard                                        */
    /*------------------------------------------------------------------*/
    #[test]
    fn directory_entry_out_of_bounds() {
        // Hand-craft an in-memory WAD: header + one directory entry.
        //
        // Header ­­­­­­­­­­­­­­­­­­­­­­­­­­­
        //  magic      "IWAD"
        //  num_lumps  1
        //  dir_offset 12  (immediately after the header)
        //
        // Directory entry ­­­­­­­­­­­­­­­­­­
        //  offset     1_000   (way past EOF)
        //  size       4
        //  name       "BAD\0\0\0\0\0"
        let mut wad = Vec::<u8>::new();
        wad.extend_from_slice(b"IWAD");
        wad.extend(&1u32.to_le_bytes()); // num_lumps
        wad.extend(&12u32.to_le_bytes()); // dir_offset

        wad.extend(&1_000u32.to_le_bytes()); // lump offset (past EOF)
        wad.extend(&4u32.to_le_bytes()); // lump size
        wad.extend(b"BAD\0\0\0\0\0"); // 8-byte name

        // Persist to a throw-away file so we can reuse `Wad::from_file`.
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        std::fs::write(tmp.path(), &wad).unwrap();

        // Loader must reject it with DirectoryOutOfBounds.
        let err = Wad::from_file(tmp.path()).unwrap_err();
        assert!(matches!(err, WadError::DirectoryOutOfBounds { .. }));
    }
}
