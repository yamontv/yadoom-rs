//! Runtime “thing” grid – a very small, cache‑friendly spatial hash.
//!
//! * One `BlockMap` cell ≙ 128×128 map‑units (vanilla constant).
//! * Each cell keeps a `SmallVec` – Doom maps rarely exceed a handful
//!   of live mobjs per block, so this is fast and allocation‑free in
//!   the common case.
//!
//! The grid is **write‑through** from the movement system:
//! `p_unset_thing_position` removes the stub from the old cell;
//! `p_set_thing_position` reinserts it at the new coords.

use glam::Vec2;
use hecs::Entity;
use smallvec::SmallVec;
use std::collections::HashMap;

use crate::world::{Aabb, Level};

use super::{ActorFlags, Class, Pos};

/*──────────────────────── core types ────────────────────────*/

/// Pre‑baked data we need during collision / AI without touching `World`.
#[derive(Clone, Copy)]
pub struct ThingSpatial {
    pub ent: Entity,
    pub pos: Pos,
    pub class: Class,
    pub flags: ActorFlags,
}

/// Row / column index in the static `BLOCKMAP` grid
pub type Bx = i32;
pub type By = i32;

/// Small fixed‑capacity cell
type Cell = SmallVec<[ThingSpatial; 8]>;

/// Hash‑map grid (sparse – only allocated where something lives)
pub struct ThingGrid {
    origin: Vec2,
    cells: HashMap<(Bx, By), Cell>,
}

/*───────────────────────── API ──────────────────────────────*/

impl ThingGrid {
    pub fn new(origin: Vec2) -> ThingGrid {
        ThingGrid {
            origin,
            cells: HashMap::new(),
        }
    }

    /// Insert / update a stub at its current BLOCKMAP coordinates.
    #[inline]
    pub fn insert(&mut self, stub: ThingSpatial) {
        let bx = Level::world_to_block(stub.pos.0.x, self.origin.x);
        let by = Level::world_to_block(stub.pos.0.y, self.origin.y);
        self.cells.entry((bx, by)).or_default().push(stub);
    }

    /// Remove the stub from the cell it used to occupy.
    ///
    /// *Call this **before** you move the actor; provide the old
    /// position so we do not need to recalculate it.*
    #[inline]
    pub fn remove(&mut self, stub: &ThingSpatial) {
        let bx = Level::world_to_block(stub.pos.0.x, self.origin.x);
        let by = Level::world_to_block(stub.pos.0.y, self.origin.y);
        if let Some(cell) = self.cells.get_mut(&(bx, by)) {
            if let Some(i) = cell.iter().position(|s| s.ent == stub.ent) {
                cell.swap_remove(i);
            }
        }
    }

    /// Visit every stub whose **origin** lies in the blocks overlapped by
    /// `[bb_min … bb_max]`.  
    /// Iteration stops early when `f` returns `false`.
    pub fn for_each_in_bbox<F>(&self, bbox: Aabb, mut f: F) -> bool
    where
        F: FnMut(&ThingSpatial) -> bool,
    {
        let xl = Level::world_to_block(bbox.min.x, self.origin.x);
        let xh = Level::world_to_block(bbox.max.x, self.origin.x);
        let yl = Level::world_to_block(bbox.min.y, self.origin.y);
        let yh = Level::world_to_block(bbox.max.y, self.origin.y);

        for bx in xl..=xh {
            for by in yl..=yh {
                if let Some(cell) = self.cells.get(&(bx, by)) {
                    for stub in cell {
                        if !f(stub) {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }
}
