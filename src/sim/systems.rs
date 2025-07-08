use glam::{Vec2, Vec3};
use hecs::World;

use crate::defs::MobjInfo;
use crate::defs::flags::MobjFlags;
use crate::defs::state::State;
use crate::world::geometry::Level;

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

pub const GRAVITY: f32 = 0.5; // 0.5 map-units / tic²   (≈ 9.8 m/s²)
pub const FRICTION: f32 = 0.875; // vanilla P_XYFriction()

pub fn physics(world: &mut World, level: &Level) {
    // we need Pos, Vel and Subsector to collide against the proper floor
    for (_, (pos, vel, ssec, class)) in
        world.query_mut::<(&mut Pos, &mut Vel, &mut Subsector, &Class)>()
    {
        /* --- 1. vertical ------------------------------------------------- */
        if !class.0.flags.contains(MobjFlags::NOGRAVITY) {
            vel.0.z -= GRAVITY; // gravity every tic
        };
        pos.1 += vel.0.z; // integrate

        // lookup the sector containing this subsector to get its floor-z
        let sector_id = level.subsectors[ssec.0 as usize].sector;
        let floor_z = level.sectors[sector_id as usize].floor_h;
        if pos.1 < floor_z {
            pos.1 = floor_z; // clamp to floor
            vel.0.z = 0.0; // lost all vertical momentum
        }
        if pos.1 == floor_z {
            vel.0.x *= FRICTION;
            vel.0.y *= FRICTION;
        }

        /* --- 2. horizontal ---------------------------------------------- */
        let old_xy = pos.0;
        pos.0 += vel.0.truncate();

        /* --- 3. subsector cache update ---------------------------------- */
        // Only pay the BSP walk if the centre actually moved.
        if pos.0 != old_xy {
            // object may have crossed a node – ask BSP to refresh its leaf
            ssec.0 = level.locate_subsector(pos.0);
        }
    }
}
