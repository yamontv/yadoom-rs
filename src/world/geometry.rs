use bitflags::bitflags;
use glam::Vec2;

use crate::world::texture::TextureId;

pub type SubsectorId = u16;
pub type LinedefId = u16;
pub type SegmentId = u16;
pub type VertexId = u16;
pub type SidedefId = u16;
pub type SectorId = u16;

/// Runtime snapshot of one map (immutable after load).
#[derive(Debug)]
pub struct Level {
    pub name: String,
    pub things: Vec<Thing>,
    pub linedefs: Vec<Linedef>,
    pub sidedefs: Vec<Sidedef>,
    pub vertices: Vec<Vertex>,
    pub segs: Vec<Seg>,
    pub subsectors: Vec<Subsector>,
    pub nodes: Vec<Node>,
    pub sectors: Vec<Sector>,
    /// lookup: subsector → sector (filled by `Level::finalise_bsp`)
    pub sector_of_subsector: Vec<u16>,
}

/*------------------------- game objects -----------------------------*/

#[derive(Clone, Debug)]
pub struct Thing {
    pub pos: Vec2,
    pub angle: f32,        // radians
    pub type_id: u16,      // mobjtype_t index
    pub min_skill: u8,     // 1 easy, 2 medium, 3 hard
    pub is_deaf: bool,     // MF_AMBUSH
    pub multiplayer: bool, // NOTSINGLE player flag
}

/*--------------------------- linedefs -------------------------------*/

bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct LinedefFlags: u16 {
        const IMPASSABLE      = 0x0001;
        const BLOCK_MONSTERS  = 0x0002;
        const TWO_SIDED       = 0x0004;
        const UPPER_UNPEGGED  = 0x0010;
        const LOWER_UNPEGGED  = 0x0020;
        const SECRET          = 0x0040;
        const BLOCK_SOUND     = 0x0080;
        const NOT_ON_MAP      = 0x0200;
        const ALREADY_ON_MAP  = 0x1000; // editor flag
    }
}

#[derive(Clone, Debug)]
pub struct Linedef {
    pub v1: VertexId,
    pub v2: VertexId,
    pub flags: LinedefFlags,
    pub special: u16,
    pub tag: u16,
    pub right_sidedef: Option<SidedefId>,
    pub left_sidedef: Option<SidedefId>,
}

/*--------------------------- sidedefs -------------------------------*/

#[derive(Clone, Debug)]
pub struct Sidedef {
    pub x_off: f32,
    pub y_off: f32,
    pub upper: TextureId, // texture names remain 8-byte arrays
    pub lower: TextureId,
    pub middle: TextureId,
    pub sector: SectorId,
}

/*----------------------- simple primitives --------------------------*/

#[derive(Clone, Copy, Debug)]
pub struct Vertex {
    pub pos: Vec2,
}

#[derive(Clone, Debug)]
pub struct Seg {
    pub v1: VertexId,
    pub v2: VertexId,
    pub linedef: LinedefId,
    pub dir: u16,
    pub offset: f32,
}

#[derive(Clone, Debug)]
pub struct Subsector {
    pub seg_count: u16,
    pub first_seg: SegmentId,
}

#[derive(Clone, Debug)]
pub struct Aabb {
    pub min: Vec2, // (x_min, z_min)
    pub max: Vec2, // (x_max, z_max)
}

#[derive(Clone, Debug)]
pub struct Node {
    pub x: f32,
    pub y: f32,
    pub dx: f32,
    pub dy: f32,
    pub bbox: [Aabb; 2],
    pub child: [u16; 2],
}

#[derive(Clone, Debug)]
pub struct Sector {
    pub floor_h: f32,
    pub ceil_h: f32,
    pub floor_tex: TextureId,
    pub ceil_tex: TextureId,
    pub light: f32,
    pub special: i16,
    pub tag: i16,
}
