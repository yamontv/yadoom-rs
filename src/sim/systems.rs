//system.rs

use crate::defs::MobjInfo;
use crate::world::geometry::Level;
use hecs::World;

use crate::defs::state::State;
use glam::{Vec2, Vec3};

/// World‑space position.  z is separate to match Doom’s 2½‑D maths.
#[derive(Debug, Clone, Copy)]
pub struct Pos(pub Vec2, pub f32);

#[derive(Debug, Clone, Copy, Default)]
pub struct Vel(pub Vec3);

#[derive(Debug, Clone, Copy)]
pub struct Angle(pub f32);

#[derive(Debug, Clone, Copy)]
pub struct Subsector(pub u16); // cached BSP leaf; 0 = unknown at spawn

#[derive(Debug, Copy, Clone)]
pub struct Class(pub &'static MobjInfo);

#[derive(Debug, Clone, Copy)]
pub struct Anim {
    pub state: State,
    pub tics: i32,
}

/* ── Animation system ─────────────────────────────────────────────── */
pub fn animation(world: &mut World) {
    for (_, anim) in world.query_mut::<&mut Anim>() {
        if anim.tics > 0 {
            anim.tics -= 1;
            if anim.tics == 0 {
                anim.state = anim.state.next();
                anim.tics = anim.state.tics();
            }
        }
    }
}

/* ── Physics placeholder ─────────────────────────────────────────── */
pub fn physics(world: &mut World, _level: &Level) {
    for (_, (pos, vel)) in world.query_mut::<(&mut Pos, &Vel)>() {
        pos.0 += vel.0.truncate();
        pos.1 += vel.0.z;
        // TODO: clamp to floor+ceiling via level geometry
    }
}
