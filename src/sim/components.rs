use glam::{Vec2, Vec3};

use crate::defs::{MobjFlags, MobjInfo, State};
use crate::world::SubsectorId;

/// World‑space position.  z is separate to match Doom’s 2½‑D maths.
#[derive(Debug, Clone, Copy)]
pub struct Position(pub Vec2, pub f32);

#[derive(Debug, Clone, Copy, Default)]
pub struct Velocity(pub Vec3);

impl Velocity {
    #[inline]
    pub fn zero_xy(&mut self) {
        self.0.x = 0.0;
        self.0.y = 0.0;
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Angle(pub f32);

#[derive(Debug, Clone, Copy)]
pub struct Subsector(pub SubsectorId);

#[derive(Debug, Copy, Clone)]
pub struct Class(pub &'static MobjInfo);

#[derive(Debug, Clone, Copy)]
pub struct Animation {
    pub state: State,
    pub tics: i32,
}

/// Player-size flag wrapper – fill in later
#[derive(Clone, Copy, Debug)]
pub struct ActorFlags(pub MobjFlags);

#[derive(Clone, Copy, Debug, Default)]
pub struct InputCmd {
    pub forward: f32,       // –1 … +1
    pub strafe: f32,        // –1 … +1  (left / right)
    pub turn: f32,          // –1 … +1  (right / left)
    pub run: bool,          // Shift
    pub fire: bool,         // Ctrl
    pub use_act: bool,      // Space
    pub weapon: Option<u8>, // 1-7 if pressed this tic
}
