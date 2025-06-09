//! Doom **map-lump parser** — builds on [`crate::wad::Wad`].
//!
//! Classic Doom stores each playable map as **eight mandatory lumps**
//! following a *zero-length marker* (`E1M1`, `MAP01`, …):
//!
//! ```text
//! [marker] THINGS LINEDEFS SIDEDEFS VERTEXES SEGS SSECTORS NODES SECTORS
//! ```
//!
//! This module adds two helpers to `Wad`:
//!
//! * `level_indices()` — discover all marker lumps.
//! * `parse_level()`   — decode a single map into strongly-typed Rust
//!   structures.

use crate::wad::Wad;
use byteorder::{LittleEndian as LE, ReadBytesExt};
use once_cell::sync::Lazy;
use regex::Regex;
use std::io::{Cursor, Read};

/// An in-world object: monster, pickup, player start, etc.
#[derive(Clone, Debug)]
pub struct Thing {
    pub x: i16,
    pub y: i16,
    pub angle: u16,
    pub type_: u16,
    pub flags: u16,
}

/// A map edge.
#[derive(Clone, Debug)]
pub struct Linedef {
    pub v1: u16,
    pub v2: u16,
    pub flags: u16,
    pub special: u16,
    pub tag: u16,
    pub right: u16,
    pub left: u16,
}

/// Texture information for one side of a linedef.
#[derive(Clone, Debug)]
pub struct Sidedef {
    pub x_off: i16,
    pub y_off: i16,
    pub upper: [u8; 8],
    pub lower: [u8; 8],
    pub middle: [u8; 8],
    pub sector: u16,
}

/// A vertex in map space.
#[derive(Clone, Debug)]
pub struct Vertex {
    pub x: i16,
    pub y: i16,
}

/// Segment (part of a linedef inside a subsector).
#[derive(Clone, Debug)]
pub struct Seg {
    pub v1: u16,
    pub v2: u16,
    pub angle: i16,
    pub linedef: u16,
    pub dir: u16,
    pub offset: i16,
}

/// BSP leaf.
#[derive(Clone, Debug)]
pub struct Subsector {
    pub seg_count: u16,
    pub first_seg: u16,
}

/// One BSP node that splits space.
#[derive(Clone, Debug)]
pub struct Node {
    pub x: i16,
    pub y: i16,
    pub dx: i16,
    pub dy: i16,
    /// [front: top, bottom, left, right] then [back: …]
    pub bbox: [[i16; 4]; 2],
    /// Child indices — bit 15 set ⇒ child is a subsector.
    pub child: [u16; 2],
}

/// A convex sector region (floor/ceiling/light).
#[derive(Clone, Debug)]
pub struct Sector {
    pub floor: i16,
    pub ceil: i16,
    pub floor_tex: [u8; 8],
    pub ceil_tex: [u8; 8],
    pub light: i16,
    pub special: i16,
    pub tag: i16,
}

/// High-level representation of a playable map.
#[derive(Clone, Debug)]
pub struct Level {
    pub things: Vec<Thing>,
    pub linedefs: Vec<Linedef>,
    pub sidedefs: Vec<Sidedef>,
    pub vertices: Vec<Vertex>,
    pub segs: Vec<Seg>,
    pub subsectors: Vec<Subsector>,
    pub nodes: Vec<Node>,
    pub sectors: Vec<Sector>,
}

/// Things that can go wrong while decoding.
#[derive(thiserror::Error, Debug)]
pub enum LevelError {
    #[error("level marker idx {0} out of range")]
    MarkerOob(usize),
    #[error("required lump {0} missing between markers")]
    Missing(&'static str),
    #[error("truncated lump {0}")]
    Truncated(&'static str),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/*=======================================================================*/
/*                     Convenience helpers on `Wad`                      */
/*=======================================================================*/
impl Wad {
    /// Return directory indices of every map marker (`E#M#`, `MAP##`).
    pub fn level_indices(&self) -> Vec<usize> {
        static RE: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"^(E[1-4]M[1-9]|MAP[0-3][0-9])$").unwrap());

        self.lumps
            .iter()
            .enumerate()
            .filter(|(_, l)| l.size == 0 && RE.is_match(Wad::lump_name(&l.name)))
            .map(|(i, _)| i)
            .collect()
    }

    /// Deserialize the eight mandatory lumps that form one map.
    pub fn parse_level(&self, marker_idx: usize) -> Result<Level, LevelError> {
        if marker_idx >= self.lumps.len() {
            return Err(LevelError::MarkerOob(marker_idx));
        }

        // ---- work out the map span (up to next zero-length lump) ----------
        let mut span_end = self.lumps.len();
        for i in marker_idx + 1..self.lumps.len() {
            if self.lumps[i].size == 0 {
                span_end = i;
                break;
            }
        }

        // locate lump name inside span → directory index
        let find = |name: &str| -> Option<usize> {
            self.lumps[marker_idx + 1..span_end]
                .iter()
                .position(|l| Wad::lump_name(&l.name) == name)
                .map(|rel| rel + marker_idx + 1)
        };

        // verify mandatory lumps
        const NEED: &[&str] = &[
            "THINGS", "LINEDEFS", "SIDEDEFS", "VERTEXES", "SEGS", "SSECTORS", "NODES", "SECTORS",
        ];
        for &n in NEED {
            if find(n).is_none() {
                return Err(LevelError::Missing(n));
            }
        }

        // helper macro: parse repetitive arrays
        macro_rules! parse_vec {
            ($buf:expr, $size:expr, $body:expr) => {{
                if $buf.len() % $size != 0 {
                    return Err(LevelError::Truncated(stringify!($body)));
                }
                let mut cur = Cursor::new($buf);
                let mut v = Vec::with_capacity($buf.len() / $size);
                while (cur.position() as usize) < $buf.len() {
                    v.push($body(&mut cur)?);
                }
                v
            }};
        }

        // ------------------------------------------------------------------
        // 1. THINGS (10 bytes)
        // ------------------------------------------------------------------
        let things = {
            let buf = self.lump_bytes(find("THINGS").unwrap());
            parse_vec!(buf, 10, |c: &mut Cursor<&[u8]>| -> std::io::Result<Thing> {
                Ok(Thing {
                    x: c.read_i16::<LE>()?,
                    y: c.read_i16::<LE>()?,
                    angle: c.read_u16::<LE>()?,
                    type_: c.read_u16::<LE>()?,
                    flags: c.read_u16::<LE>()?,
                })
            })
        };

        // 2. LINEDEFS (14 bytes)
        let linedefs = {
            let buf = self.lump_bytes(find("LINEDEFS").unwrap());
            parse_vec!(
                buf,
                14,
                |c: &mut Cursor<&[u8]>| -> std::io::Result<Linedef> {
                    Ok(Linedef {
                        v1: c.read_u16::<LE>()?,
                        v2: c.read_u16::<LE>()?,
                        flags: c.read_u16::<LE>()?,
                        special: c.read_u16::<LE>()?,
                        tag: c.read_u16::<LE>()?,
                        right: c.read_u16::<LE>()?,
                        left: c.read_u16::<LE>()?,
                    })
                }
            )
        };

        // 3. SIDEDEFS (30 bytes)
        let sidedefs = {
            let buf = self.lump_bytes(find("SIDEDEFS").unwrap());
            let read_tex = |c: &mut Cursor<&[u8]>| -> std::io::Result<[u8; 8]> {
                let mut t = [0u8; 8];
                c.read_exact(&mut t)?;
                Ok(t)
            };
            parse_vec!(
                buf,
                30,
                |c: &mut Cursor<&[u8]>| -> std::io::Result<Sidedef> {
                    Ok(Sidedef {
                        x_off: c.read_i16::<LE>()?,
                        y_off: c.read_i16::<LE>()?,
                        upper: read_tex(c)?,
                        lower: read_tex(c)?,
                        middle: read_tex(c)?,
                        sector: c.read_u16::<LE>()?,
                    })
                }
            )
        };

        // 4. VERTEXES (4 bytes)
        let vertices = {
            let buf = self.lump_bytes(find("VERTEXES").unwrap());
            parse_vec!(buf, 4, |c: &mut Cursor<&[u8]>| -> std::io::Result<Vertex> {
                Ok(Vertex {
                    x: c.read_i16::<LE>()?,
                    y: c.read_i16::<LE>()?,
                })
            })
        };

        // 5. SEGS (12 bytes)
        let segs = {
            let buf = self.lump_bytes(find("SEGS").unwrap());
            parse_vec!(buf, 12, |c: &mut Cursor<&[u8]>| -> std::io::Result<Seg> {
                Ok(Seg {
                    v1: c.read_u16::<LE>()?,
                    v2: c.read_u16::<LE>()?,
                    angle: c.read_i16::<LE>()?,
                    linedef: c.read_u16::<LE>()?,
                    dir: c.read_u16::<LE>()?,
                    offset: c.read_i16::<LE>()?,
                })
            })
        };

        // 6. SSECTORS (4 bytes)
        let subsectors = {
            let buf = self.lump_bytes(find("SSECTORS").unwrap());
            parse_vec!(
                buf,
                4,
                |c: &mut Cursor<&[u8]>| -> std::io::Result<Subsector> {
                    Ok(Subsector {
                        seg_count: c.read_u16::<LE>()?,
                        first_seg: c.read_u16::<LE>()?,
                    })
                }
            )
        };

        // 7. NODES (28 bytes)
        let nodes = {
            let buf = self.lump_bytes(find("NODES").unwrap());
            parse_vec!(buf, 28, |c: &mut Cursor<&[u8]>| -> std::io::Result<Node> {
                Ok(Node {
                    x: c.read_i16::<LE>()?,
                    y: c.read_i16::<LE>()?,
                    dx: c.read_i16::<LE>()?,
                    dy: c.read_i16::<LE>()?,
                    bbox: [
                        [
                            c.read_i16::<LE>()?,
                            c.read_i16::<LE>()?,
                            c.read_i16::<LE>()?,
                            c.read_i16::<LE>()?,
                        ],
                        [
                            c.read_i16::<LE>()?,
                            c.read_i16::<LE>()?,
                            c.read_i16::<LE>()?,
                            c.read_i16::<LE>()?,
                        ],
                    ],
                    child: [c.read_u16::<LE>()?, c.read_u16::<LE>()?],
                })
            })
        };

        // 8. SECTORS (26 bytes)
        let sectors = {
            let buf = self.lump_bytes(find("SECTORS").unwrap());
            let read_tex = |c: &mut Cursor<&[u8]>| -> std::io::Result<[u8; 8]> {
                let mut t = [0u8; 8];
                c.read_exact(&mut t)?;
                Ok(t)
            };
            parse_vec!(
                buf,
                26,
                |c: &mut Cursor<&[u8]>| -> std::io::Result<Sector> {
                    Ok(Sector {
                        floor: c.read_i16::<LE>()?,
                        ceil: c.read_i16::<LE>()?,
                        floor_tex: read_tex(c)?,
                        ceil_tex: read_tex(c)?,
                        light: c.read_i16::<LE>()?,
                        special: c.read_i16::<LE>()?,
                        tag: c.read_i16::<LE>()?,
                    })
                }
            )
        };

        Ok(Level {
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

    /// Locate `assets/doom1.wad` relative to crate root.
    fn doom_wad() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("doom1.wad")
    }

    #[test]
    fn first_map_parses() {
        let wad = Wad::from_file(doom_wad()).expect("doom1.wad");
        let first_marker = wad
            .level_indices()
            .first()
            .copied()
            .expect("no map markers found");
        let level = wad.parse_level(first_marker).expect("parse");
        assert!(level.vertices.len() > 100, "suspiciously small map");
        assert_eq!(level.things[0].type_, 1, "player 1 start missing?");
    }
}
