use super::{Angle, Anim, Class, Pos, Subsector, Vel};
use crate::defs::MobjInfo;
use glam::{Vec2, Vec3};
use hecs::World;

pub fn spawn_mobj(
    world: &mut World,
    info: &'static MobjInfo,
    x: f32,
    y: f32,
    z: f32,
    angle: f32,
    subsector: u16,
) -> hecs::Entity {
    world.spawn((
        Pos(Vec2::new(x, y), z),
        Vel(Vec3::ZERO),
        Angle(angle),
        Subsector(subsector),
        Anim {
            state: info.spawnstate,
            tics: info.spawnstate.tics(),
        },
        Class(info),
    ))
}
