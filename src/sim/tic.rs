use super::{mob, systems};
use crate::world::Level;
use hecs::World;
use std::time::{Duration, Instant};

pub const SIM_FPS: u32 = 35;
pub const DT: f32 = 1.0 / SIM_FPS as f32;
const TIC: Duration = Duration::from_micros(1_000_000 / SIM_FPS as u64);

/// Owns the ECS world and drives all game‑logic systems.
pub struct TicRunner {
    world: World,
    last: Instant,
}

impl Default for TicRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl TicRunner {
    pub fn new() -> Self {
        Self {
            world: World::new(),
            last: Instant::now(),
        }
    }

    #[inline]
    pub fn world(&self) -> &hecs::World {
        &self.world
    }

    #[inline]
    pub fn world_mut(&mut self) -> &mut hecs::World {
        &mut self.world
    }

    /// Spawn a monster/item entity and return its `Entity` handle.
    #[inline]
    pub fn spawn_mobj(
        &mut self,
        level: &Level,
        info: &'static crate::defs::MobjInfo,
        x: f32,
        y: f32,
        angle: f32,
        subsector: u16,
    ) -> hecs::Entity {
        mob::spawn_mobj(&mut self.world, level, info, x, y, angle, subsector)
    }

    /// Advance enough tics to synchronise simulation with real time.
    pub fn pump(&mut self, level: &Level) {
        while self.last.elapsed() >= TIC {
            self.tick(level);
            self.last += TIC;
        }
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
