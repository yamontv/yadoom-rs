use super::{Angle, Anim, Class, Pos, Subsector, Vel};
use crate::defs::{MobjInfo, flags::MobjFlags};
use crate::world::geometry::Level;
use glam::{Vec2, Vec3};
use hecs::World;

// sim/num.rs  (or inside mob.rs)
pub fn xy_from_speed(speed: f32, angle_rad: f32) -> glam::Vec2 {
    // Doom’s map units: east = 0 rad, counter-clockwise positive
    glam::Vec2::new(angle_rad.cos(), angle_rad.sin()) * speed
}

pub fn spawn_mobj(
    world: &mut World,
    level: &Level,
    info: &'static MobjInfo,
    x: f32,
    y: f32,
    angle: f32,
    subsector: u16,
) -> hecs::Entity {
    let speed = info.speed; // map-units per tic

    let sec_idx = level.subsectors[subsector as usize].sector;
    let sector = &level.sectors[sec_idx as usize];

    let z = if info.flags.contains(MobjFlags::SPAWNCEILING) {
        sector.ceil_h - (info.height as f32)
    } else {
        sector.floor_h
    };

    let vel = if speed > 0 {
        let v = xy_from_speed(speed as f32, angle);
        Vec3::new(v.x, v.y, 0.0)
    } else {
        Vec3::ZERO
    };

    world.spawn((
        Pos(Vec2::new(x, y), z),
        Vel(vel),
        Angle(angle),
        Subsector(subsector),
        Anim {
            state: info.spawnstate,
            tics: info.spawnstate.tics(),
        },
        Class(info),
    ))
}
