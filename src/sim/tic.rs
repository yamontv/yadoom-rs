// tic.rs
use super::{mob, systems};
use crate::world::geometry::Level;
use hecs::QueryBorrow;
use hecs::World;
use std::time::{Duration, Instant};

pub const SIM_FPS: u32 = 35;
const TIC: Duration = Duration::from_micros(1_000_000 / SIM_FPS as u64);

/// Owns the ECS world and drives all game‑logic systems.
pub struct TicRunner {
    world: World,
    last: Instant,
}

impl TicRunner {
    pub fn new() -> Self {
        Self {
            world: World::new(),
            last: Instant::now(),
        }
    }

    /// Spawn a monster/item entity and return its `Entity` handle.
    #[inline]
    pub fn spawn_mobj(
        &mut self,
        info: &'static crate::defs::MobjInfo,
        x: f32,
        y: f32,
        z: f32,
        angle: f32,
        subsector: u16,
    ) -> hecs::Entity {
        mob::spawn_mobj(&mut self.world, info, x, y, z, angle, subsector)
    }

    /// Advance enough tics to synchronise simulation with real time.
    pub fn pump(&mut self, level: &Level) {
        while self.last.elapsed() >= TIC {
            self.tick(level);
            self.last += TIC;
        }
    }

    /* ---------------------------------------------------------------- */
    /* accessors for the renderer                                        */
    /* ---------------------------------------------------------------- */
    #[inline]
    pub fn mobjs(&self) -> QueryBorrow<(&systems::Pos, &systems::Anim)> {
        self.world.query::<(&systems::Pos, &systems::Anim)>() // caller can `.iter()` or `.into_iter()`
    }

    /* ---------------------------------------------------------------- */
    /* internal: run one fixed‑rate game tic                             */
    /* ---------------------------------------------------------------- */
    fn tick(&mut self, level: &Level) {
        systems::animation(&mut self.world);
        systems::physics(&mut self.world, level);
        // TODO: AI, door, platform systems go here.
    }
}
