//! # Doom WAD loader
//!
//! * Reads the entire IWAD into RAM.  
//! * Provides zero-copy access to individual lumps.  
//! * Decodes binary lumps into typed vectors with **bincode 2**.
//!
//! Only the “IWAD” magic is accepted for now (PWAD support can be added
//! later).

use bincode::{Decode, config, decode_from_slice};
use byteorder::{LittleEndian as LE, ReadBytesExt};
use std::{
    collections::HashMap,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    mem,
    path::Path,
};
use thiserror::Error;

/// One entry in the lump directory (16 bytes on disk).
#[derive(Clone, Debug)]
pub struct LumpInfo {
    pub name: [u8; 8],
    pub offset: u32,
    pub size: u32,
}

/// Entire WAD in memory (raw bytes + parsed directory).
#[derive(Debug)]
pub struct Wad {
    lumps: Vec<LumpInfo>,
    bytes: Vec<u8>,
    by_name: HashMap<String, usize>,
}

/// Loader / decoding errors.
#[derive(Error, Debug)]
pub enum WadError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("file is not an IWAD")]
    BadMagic,

    #[error("directory extends beyond end of file")]
    DirectoryOutOfBounds,

    #[error("lump index {0} out of range")]
    BadIndex(usize),

    #[error("lump {name} (# {index}) slice {offset}+{size} past EOF ({file_size})")]
    BadOffset {
        index: usize,
        name: String,
        offset: u32,
        size: u32,
        file_size: usize,
    },

    #[error("lump {name} (# {index}) size {size} not multiple of element {elem_size}")]
    BadLumpSize {
        index: usize,
        name: String,
        size: usize,
        elem_size: usize,
    },

    #[error("lump {name} (# {index}) element {elem}: {source}")]
    BadElement {
        index: usize,
        name: String,
        elem: usize,
        source: bincode::error::DecodeError,
    },
}

impl Wad {
    // ------------------------------------------------------------------ //
    // Low-level helpers
    // ------------------------------------------------------------------ //

    /// Expose directory as a read-only slice
    pub fn lumps(&self) -> &[LumpInfo] {
        &self.lumps
    }

    /// Return &str view of an 8-byte lump name (trimmed at first NUL).
    pub fn lump_name_str(name: &[u8; 8]) -> &str {
        let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
        std::str::from_utf8(&name[..end]).unwrap_or("?")
    }

    /// Raw bytes of lump `idx` (slice into `self.bytes`).
    pub fn lump_bytes(&self, idx: usize) -> Result<&[u8], WadError> {
        let l = self.lumps.get(idx).ok_or(WadError::BadIndex(idx))?;
        let start = l.offset as usize;
        let end = start + l.size as usize;
        if end > self.bytes.len() {
            return Err(WadError::BadOffset {
                index: idx,
                name: Self::lump_name_str(&l.name).into(),
                offset: l.offset,
                size: l.size,
                file_size: self.bytes.len(),
            });
        }
        Ok(&self.bytes[start..end])
    }

    /// Find the last lump with `name` (case-sensitive like vanilla Doom).
    pub fn find_lump(&self, name: &str) -> Option<usize> {
        self.by_name.get(name).copied()
    }

    // ------------------------------------------------------------------ //
    // Generic decode helper
    // ------------------------------------------------------------------ //

    pub fn lump_to_vec<T>(&self, idx: usize) -> Result<Vec<T>, WadError>
    where
        T: Decode<()>,
    {
        let bytes = self.lump_bytes(idx)?;
        let elem = mem::size_of::<T>();

        if bytes.is_empty() || bytes.len() % elem != 0 {
            return Err(WadError::BadLumpSize {
                index: idx,
                name: Self::lump_name_str(&self.lumps[idx].name).into(),
                size: bytes.len(),
                elem_size: elem,
            });
        }

        let cfg = config::standard()
            .with_fixed_int_encoding()
            .with_little_endian();
        let mut out = Vec::with_capacity(bytes.len() / elem);
        let mut slice = bytes;

        while !slice.is_empty() {
            let (val, read) = decode_from_slice::<T, _>(slice, cfg) // ← only 2 generics / 2 args
                .map_err(|e| WadError::BadElement {
                    index: idx,
                    name: Self::lump_name_str(&self.lumps[idx].name).into(),
                    elem: bytes.len(),
                    source: e,
                })?;
            out.push(val);
            slice = &slice[read..];
        }
        Ok(out)
    }

    // ------------------------------------------------------------------ //
    // Loading
    // ------------------------------------------------------------------ //

    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, WadError> {
        let mut file = File::open(path)?;

        let mut magic = [0u8; 4];
        file.read_exact(&mut magic)?;
        if &magic != b"IWAD" {
            return Err(WadError::BadMagic);
        }

        let num_lumps = file.read_u32::<LE>()?;
        let dir_offset = file.read_u32::<LE>()?;

        // read whole file
        let mut bytes = Vec::new();
        file.seek(SeekFrom::Start(0))?;
        file.read_to_end(&mut bytes)?;

        // directory bounds check
        let dir_end = dir_offset as usize + num_lumps as usize * 16;
        if dir_end > bytes.len() {
            return Err(WadError::DirectoryOutOfBounds);
        }

        // parse directory
        let mut lumps = Vec::with_capacity(num_lumps as usize);
        let mut cur = &bytes[dir_offset as usize..dir_end];

        for _ in 0..num_lumps {
            let off = cur.read_u32::<LE>()?;
            let size = cur.read_u32::<LE>()?;
            let mut name = [0u8; 8];
            cur.read_exact(&mut name)?;
            lumps.push(LumpInfo {
                name,
                offset: off,
                size,
            });
        }

        // validate each lump slice
        for (i, l) in lumps.iter().enumerate() {
            let end = l.offset as usize + l.size as usize;
            if end > bytes.len() {
                return Err(WadError::BadOffset {
                    index: i,
                    name: Self::lump_name_str(&l.name).into(),
                    offset: l.offset,
                    size: l.size,
                    file_size: bytes.len(),
                });
            }
        }

        // build name → idx map (later lumps shadow earlier ones)
        let mut by_name = HashMap::with_capacity(lumps.len());
        for (i, l) in lumps.iter().enumerate().rev() {
            by_name
                .entry(Self::lump_name_str(&l.name).to_owned())
                .or_insert(i);
        }

        Ok(Self {
            lumps,
            bytes,
            by_name,
        })
    }
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn doom_wad() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("doom.wad")
    }

    #[test]
    fn opens_header() {
        let wad = Wad::from_file(doom_wad()).unwrap();
        assert!(wad.lumps.len() > 100);
    }

    #[test]
    fn find_lump_by_name() {
        let wad = Wad::from_file(doom_wad()).unwrap();
        let idx = wad.find_lump("PLAYPAL").expect("PLAYPAL not found");
        assert_eq!(Wad::lump_name_str(&wad.lumps[idx].name), "PLAYPAL");
    }

    #[test]
    fn titlepic_reasonable_size() {
        let wad = Wad::from_file(doom_wad()).unwrap();
        let idx = wad.find_lump("TITLEPIC").unwrap();
        let size = wad.lumps[idx].size;
        assert!((64_000..200_000).contains(&size));
    }

    #[test]
    fn byte_slice_len_matches_dir() {
        let wad = Wad::from_file(doom_wad()).unwrap();
        for (i, l) in wad.lumps.iter().enumerate() {
            assert_eq!(wad.lump_bytes(i).unwrap().len() as u32, l.size);
        }
    }

    #[test]
    fn lump_to_vec_roundtrip() {
        #[repr(C)]
        #[derive(Clone, Copy, Debug, PartialEq, bincode::Decode)]
        struct Foo {
            a: i16,
            b: i16,
        }

        // hand-craft lump [ (1,2), (3,4) ]
        let bytes = [1i16, 2, 3, 4]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();

        let wad = Wad {
            lumps: vec![LumpInfo {
                name: *b"FOO\0\0\0\0\0",
                offset: 12,
                size: bytes.len() as u32,
            }],
            bytes: {
                let mut v = vec![0u8; 12];
                v.extend(&bytes);
                v
            },
            by_name: HashMap::new(),
        };

        let v: Vec<Foo> = wad.lump_to_vec(0).unwrap();
        assert_eq!(v, vec![Foo { a: 1, b: 2 }, Foo { a: 3, b: 4 }]);
    }
}
