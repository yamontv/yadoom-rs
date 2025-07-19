use super::{
    ActorFlags, Angle, Animation, Class, Position, Subsector, ThingGrid, ThingSpatial, Velocity,
};
use crate::defs::{MobjInfo, flags::MobjFlags};
use crate::world::Level;
use glam::{Vec2, Vec3};
use hecs::World;

pub fn spawn_mobj(
    world: &mut World,
    thing_grid: &mut ThingGrid,
    level: &Level,
    info: &'static MobjInfo,
    x: f32,
    y: f32,
    angle: f32,
    subsector: u16,
) -> hecs::Entity {
    let sec_idx = level.subsectors[subsector as usize].sector;
    let sector = &level.sectors[sec_idx as usize];

    let z = if info.flags.contains(MobjFlags::SPAWNCEILING) {
        sector.ceil_h - (info.height as f32)
    } else {
        sector.floor_h
    };

    let pos = Position(Vec2::new(x, y), z);
    let class = Class(info);
    let flags = ActorFlags(info.flags);

    let ent = world.spawn((
        flags,
        pos,
        Velocity(Vec3::ZERO),
        Angle(angle),
        Subsector(subsector),
        Animation {
            state: info.spawnstate,
            tics: info.spawnstate.tics(),
        },
        class,
    ));

    if !flags.0.contains(MobjFlags::NOBLOCKMAP) {
        thing_grid.insert(ThingSpatial {
            ent,
            pos,
            class,
            flags,
        });
    }

    ent
}
