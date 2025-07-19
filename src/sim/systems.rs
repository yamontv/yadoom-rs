use hecs::World;

use super::{Angle, Animation, InputCmd, ThingGrid, Velocity, tic::DT, xy_movement_system};
use crate::world::Level;

/* ── Animation system ─────────────────────────────────────────────── */
pub fn animation(world: &mut World) {
    for (_, anim) in world.query_mut::<&mut Animation>() {
        if anim.tics > 0 {
            anim.tics -= 1;
            if anim.tics == 0 {
                anim.state = anim.state.next();
                anim.tics = anim.state.tics();
            }
        }
    }
}

pub fn physics(world: &mut World, thing_grid: &mut ThingGrid, level: &Level) {
    xy_movement_system(world, thing_grid, level);
}

pub const MOVE_SPEED: f32 = 250.0; // map-units / second
pub const TURN_RATE: f32 = std::f32::consts::PI; // rad / second (180°/s)
pub fn player_input(world: &mut World, player: hecs::Entity, cmd: InputCmd) {
    if let Ok(mut q) = world.query_one::<(&mut Angle, &mut Velocity)>(player) {
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
