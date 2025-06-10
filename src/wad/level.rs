use crate::wad::{Wad, WadError};
use bincode::Decode;
use once_cell::sync::Lazy;
use regex::Regex;

/*=======================================================================*/
/*                         Raw binary structs                            */
/*=======================================================================*/

#[repr(C)]
#[derive(Clone, Copy, Decode, Debug)]
pub struct RawThing {
    pub x: i16,
    pub y: i16,
    pub angle: i16,
    pub type_: i16,
    pub options: i16,
}

#[repr(C)]
#[derive(Clone, Copy, Decode, Debug)]
pub struct RawLinedef {
    pub v1: i16,
    pub v2: i16,
    pub flags: i16,
    pub special: i16,
    pub tag: i16,
    pub sidenum: [i16; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Decode, Debug)]
pub struct RawSidedef {
    pub x_off: i16,
    pub y_off: i16,
    pub top_tex: [u8; 8],
    pub bottom_tex: [u8; 8],
    pub mid_tex: [u8; 8],
    pub sector: i16,
}

#[repr(C)]
#[derive(Clone, Copy, Decode, Debug)]
pub struct RawVertex {
    pub x: i16,
    pub y: i16,
}

#[repr(C)]
#[derive(Clone, Copy, Decode, Debug)]
pub struct RawSeg {
    pub v1: i16,
    pub v2: i16,
    pub angle: i16,
    pub linedef: i16,
    pub side: i16,
    pub offset: i16,
}

#[repr(C)]
#[derive(Clone, Copy, Decode, Debug)]
pub struct RawSubsector {
    pub seg_count: i16,
    pub first_seg: i16,
}

#[repr(C)]
#[derive(Clone, Copy, Decode, Debug)]
pub struct RawNode {
    pub x: i16,
    pub y: i16,
    pub dx: i16,
    pub dy: i16,
    pub bbox: [[i16; 4]; 2],
    pub child: [u16; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Decode, Debug)]
pub struct RawSector {
    pub floor_h: i16,
    pub ceil_h: i16,
    pub floor_tex: [u8; 8],
    pub ceil_tex: [u8; 8],
    pub light: i16,
    pub special: i16,
    pub tag: i16,
}

/*=======================================================================*/
/*                     Aggregate returned by `parse_level`               */
/*=======================================================================*/
#[derive(Debug)]
pub struct RawLevel {
    pub name: String,
    pub things: Vec<RawThing>,
    pub linedefs: Vec<RawLinedef>,
    pub sidedefs: Vec<RawSidedef>,
    pub vertices: Vec<RawVertex>,
    pub segs: Vec<RawSeg>,
    pub subsectors: Vec<RawSubsector>,
    pub nodes: Vec<RawNode>,
    pub sectors: Vec<RawSector>,
}

/*=======================================================================*/
/*                                Errors                                 */
/*=======================================================================*/

#[derive(Debug, thiserror::Error)]
pub enum LevelError {
    #[error("marker index {0} out of bounds")]
    MarkerOob(usize),

    #[error("expected lump `{0}` not found after level marker")]
    Missing(&'static str),

    #[error(transparent)]
    Wad(#[from] WadError),
}

/*=======================================================================*/
/*                     Convenience helpers on `Wad`                      */
/*=======================================================================*/
impl Wad {
    /// Return directory indices of every map marker (`E#M#`, `MAP##`).
    pub fn level_indices(&self) -> Vec<usize> {
        static RE: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"^(E[1-4]M[1-9]|MAP[0-3][0-9])$").unwrap());

        self.lumps()
            .iter()
            .enumerate()
            .filter(|(_, l)| l.size == 0 && RE.is_match(Self::lump_name_str(&l.name)))
            .map(|(i, _)| i)
            .collect()
    }

    /// Return the index of the lump `name` **immediately after** `start`.
    fn idx_of(&self, start: usize, name: &'static str) -> Result<usize, LevelError> {
        let l = self.lumps().get(start).ok_or(LevelError::Missing(name))?;
        match Self::lump_name_str(&l.name) == name {
            true => Ok(start),
            false => Err(LevelError::Missing(name)),
        }
    }

    /// Decode the eight mandatory lumps that make up a classic Doom map.
    pub fn parse_level(&self, marker_idx: usize) -> Result<RawLevel, LevelError> {
        // --- bounds check on marker index --------------------------------
        if marker_idx >= self.lumps().len() {
            return Err(LevelError::MarkerOob(marker_idx));
        }

        // --- fixed lump order after marker -------------------------------
        let things_idx = self.idx_of(marker_idx + 1, "THINGS")?;
        let linedefs_idx = self.idx_of(marker_idx + 2, "LINEDEFS")?;
        let sidedefs_idx = self.idx_of(marker_idx + 3, "SIDEDEFS")?;
        let vertices_idx = self.idx_of(marker_idx + 4, "VERTEXES")?;
        let segs_idx = self.idx_of(marker_idx + 5, "SEGS")?;
        let ssectors_idx = self.idx_of(marker_idx + 6, "SSECTORS")?;
        let nodes_idx = self.idx_of(marker_idx + 7, "NODES")?;
        let sectors_idx = self.idx_of(marker_idx + 8, "SECTORS")?;
        // REJECT / BLOCKMAP can be skipped while prototyping

        // --- decode each lump -------------------------------------------
        let things = self.lump_to_vec::<RawThing>(things_idx)?;
        let linedefs = self.lump_to_vec::<RawLinedef>(linedefs_idx)?;
        let sidedefs = self.lump_to_vec::<RawSidedef>(sidedefs_idx)?;
        let vertices = self.lump_to_vec::<RawVertex>(vertices_idx)?;
        let segs = self.lump_to_vec::<RawSeg>(segs_idx)?;
        let subsectors = self.lump_to_vec::<RawSubsector>(ssectors_idx)?;
        let nodes = self.lump_to_vec::<RawNode>(nodes_idx)?;
        let sectors = self.lump_to_vec::<RawSector>(sectors_idx)?;

        Ok(RawLevel {
            name: Self::lump_name_str(&self.lumps()[marker_idx].name).into(),
            things,
            linedefs,
            sidedefs,
            vertices,
            segs,
            subsectors,
            nodes,
            sectors,
        })
    }
}

/*=======================================================================*/
/*                                Tests                                  */
/*=======================================================================*/
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn doom_wad() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/doom1.wad")
    }

    #[test]
    fn first_map_parses() {
        let wad = Wad::from_file(doom_wad()).unwrap();
        let m0 = wad.level_indices()[0];
        let lvl = wad.parse_level(m0).expect("level decode");
        assert!(lvl.vertices.len() > 100);
        assert_eq!(lvl.things.first().unwrap().type_, 1); // Player 1 start
    }

    #[test]
    fn bad_marker_oob() {
        let wad = Wad::from_file(doom_wad()).unwrap();
        let err = wad.parse_level(wad.lumps().len() + 10).unwrap_err();
        matches!(err, LevelError::MarkerOob(_));
    }

    #[test]
    fn missing_things_guard() {
        let wad = Wad::from_file(doom_wad()).unwrap();
        // Pick the second lump after the map marker (= LINEDEFS),
        // parse_level should complain that THINGS is missing.
        let idx = wad.level_indices()[0] + 2;
        let err = wad.parse_level(idx).unwrap_err();
        matches!(err, LevelError::Missing("THINGS"));
    }
}
