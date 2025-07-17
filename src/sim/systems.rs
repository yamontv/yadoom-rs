use glam::{Vec2, Vec3};
use hecs::World;

use super::{tic::DT, xy_movement_system, spacial::ThingGrid};
use crate::defs::MobjInfo;
use crate::defs::flags::MobjFlags;
use crate::defs::state::State;
use crate::world::Level;

/// World‑space position.  z is separate to match Doom’s 2½‑D maths.
#[derive(Debug, Clone, Copy)]
pub struct Pos(pub Vec2, pub f32);

#[derive(Debug, Clone, Copy, Default)]
pub struct Vel(pub Vec3);

impl Vel {
    #[inline]
    pub fn zero_xy(&mut self) {
        self.0.x = 0.0;
        self.0.y = 0.0;
    }
}

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

// pub const GRAVITY: f32 = 0.5; // 0.5 map-units / tic²   (≈ 9.8 m/s²)
// pub const FRICTION: f32 = 0.875; // vanilla P_XYFriction()
pub const MOVE_SPEED: f32 = 250.0; // map-units / second
pub const TURN_RATE: f32 = std::f32::consts::PI; // rad / second (180°/s)
pub const MAX_STEP_HEIGHT: f32 = 24.0; // max step 24 mu without a jump button */
pub fn physics(world: &mut World, thing_grid: &mut ThingGrid, level: &Level) {
    xy_movement_system(world, thing_grid, level);
    // for (_, (pos, vel, ssec, class)) in
    //     world.query_mut::<(&mut Pos, &mut Vel, &mut Subsector, &Class)>()
    // {
    //     /* ------------------------------------------------------- XY move */
    //     let MoveResult {
    //         pos: new_xy,
    //         subsector: new_ss,
    //         hit_wall,
    //     } = slide_move(level, ssec.0, pos.0, vel.0.truncate(), class);

    //     pos.0 = new_xy;
    //     ssec.0 = new_ss;

    //     /* ------------------------------------------------------- look up sector heights */
    //     let sector_id = level.subsectors[ssec.0 as usize].sector as usize;
    //     let sector = &level.sectors[sector_id];
    //     let floor_z = sector.floor_h;
    //     let ceil_z = sector.ceil_h;

    //     /* ------------------------------------------------------- Z move  */
    //     if !class.0.flags.contains(MobjFlags::NOGRAVITY) {
    //         vel.0.z -= GRAVITY;
    //     }
    //     pos.1 += vel.0.z;

    //     /* --------------- clamp to floor (with step-up help) ------------- */
    //     if pos.1 < floor_z {
    //         // below floor → snap + kill momentum
    //         pos.1 = floor_z;
    //         vel.0.z = 0.0;
    //     } else {
    //         // try to *step up* small rises (stairs, ledges)
    //         let delta = pos.1 - floor_z;
    //         if 0.0 < delta && delta < MAX_STEP_HEIGHT {
    //             pos.1 = floor_z; // gently slide onto the step
    //         }
    //     }

    //     /* --------------- clamp to ceiling ------------------------------- */
    //     if pos.1 > ceil_z {
    //         pos.1 = ceil_z;
    //         vel.0.z = 0.0;
    //     }

    //     /* --------------- friction only when on ground ------------------- */
    //     if (pos.1 - floor_z).abs() < f32::EPSILON {
    //         vel.0.x *= FRICTION;
    //         vel.0.y *= FRICTION;
    //     }
    // }
}

pub fn player_input(world: &mut World, player: hecs::Entity, cmd: InputCmd) {
    if let Ok(mut q) = world.query_one::<(&mut Angle, &mut Vel)>(player) {
        if let Some((ang, vel)) = q.get() {
            /* 1. turn (scaled inside system) */
            if cmd.turn != 0.0 {
                ang.0 = (ang.0 + cmd.turn * TURN_RATE * DT).rem_euclid(std::f32::consts::TAU);
            }

            let speed = if cmd.run {
                MOVE_SPEED * 1.5
            } else {
                MOVE_SPEED
            };

            /* 2. wish-vel (scaled inside system) */
            if cmd.forward != 0.0 || cmd.strafe != 0.0 {
                let (s, c) = ang.0.sin_cos();
                let fwd = glam::Vec2::new(c, s);
                let right = fwd.perp();
                let dir = (fwd * cmd.forward) - (right * cmd.strafe);
                let wish = dir.normalize_or_zero();

                vel.0.x = wish.x * speed * DT;
                vel.0.y = wish.y * speed * DT;
            } else {
                vel.zero_xy();
            }

            if cmd.fire {
                println!("FIRE!");
            }
            if cmd.use_act {
                println!("USE / OPEN");
            }
            if let Some(w) = cmd.weapon {
                println!("select weapon {}", w);
            }
        }
    }
}
