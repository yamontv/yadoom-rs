//! Doom-style XY movement for the *hecs* ECS.
//!
//! Vanilla function order is kept so you can bring over more helpers
//! verbatim – every un-ported call is still a plain `todo!()`.

use glam::{Vec2, Vec3};
use hecs::{Entity, World};
use smallvec::SmallVec;

use super::{ActorFlags, Anim, Class, Pos, Subsector, Vel};
use crate::defs::{State, flags::MobjFlags};
use crate::world::{Aabb, Level, Linedef, LinedefFlags, LinedefId};

/* ----------------------------------------------------------------- */
/*  Physics constants (f32 map-units)                                */
/* ----------------------------------------------------------------- */
const MAX_MOVE: f32 = 32.0; // vanilla 0x10000
const MAX_STEP_HEIGHT: f32 = 24.0; // vanilla 24*FRACUNIT
const STOP_SPEED: f32 = 0.125; // vanilla FRACUNIT/8
const FRICTION: f32 = 0.90625; // vanilla 0xE800/FRACUNIT

/* ----------------------------------------------------------------- */
/*  Action queue – avoids mutable-borrow conflicts                    */
/* ----------------------------------------------------------------- */
enum Action {
    SetState { entity: Entity, new_state: State },
    Explode { entity: Entity },
}
type Actions = SmallVec<[Action; 2]>;

/* ================================================================= */
/*  Public system                                                    */
/* ================================================================= */

pub fn xy_movement_system(world: &mut World, level: &Level) {
    let mut queue = Actions::new();

    {
        let query = world.query_mut::<(
            &mut Pos,
            &mut Vel,
            &mut ActorFlags,
            &Class,
            &mut Subsector,
            &mut Anim,
        )>();

        for (e, (p, v, f, c, ss, an)) in query {
            queue.extend(p_xy_movement(level, e, p, v, f, c, ss, an));
        }
    }

    // side-effect phase
    for act in queue {
        match act {
            Action::SetState { entity, new_state } => p_set_mobj_state(world, entity, new_state),
            Action::Explode { entity } => p_explode_missile(world, entity, level),
        }
    }
}

/* ================================================================= */
/*  Core P_XYMovement                                                */
/* ================================================================= */

#[allow(clippy::too_many_arguments)]
fn p_xy_movement(
    level: &Level,
    ent: Entity,
    pos: &mut Pos,
    vel: &mut Vel,
    flags: &mut ActorFlags,
    class: &Class,
    subsector: &mut Subsector,
    anim: &mut Anim,
) -> Actions {
    let mut acts = Actions::new();

    /* -- 0: zero momentum ------------------------------------------ */
    if vel.0.x == 0.0 && vel.0.y == 0.0 {
        if flags.0.contains(MobjFlags::SKULLFLY) {
            flags.0.remove(MobjFlags::SKULLFLY);
            vel.0 = Vec3::ZERO;
            acts.push(Action::SetState {
                entity: ent,
                new_state: class.0.spawnstate,
            });
        }
        return acts;
    }

    let is_player = class.0.id == "PLAYER";

    /* -- 1: clamp & prepare move ----------------------------------- */
    vel.0.x = vel.0.x.clamp(-MAX_MOVE, MAX_MOVE);
    vel.0.y = vel.0.y.clamp(-MAX_MOVE, MAX_MOVE);
    let (mut xmove, mut ymove) = (vel.0.x, vel.0.y);

    /* -- 2: possibly split the move (vanilla does this recursively) */
    while xmove != 0.0 || ymove != 0.0 {
        let mut step = Vec2::new(xmove, ymove);
        if step.x.abs() > MAX_MOVE * 0.5 || step.y.abs() > MAX_MOVE * 0.5 {
            step *= 0.5; // halve once – good enough for map speeds
        }
        xmove -= step.x;
        ymove -= step.y;

        let dest = pos.0 + step;
        let mut slide_normal = None;

        if !p_try_move(
            level,
            pos,
            subsector,
            class,
            is_player,
            dest,
            &mut slide_normal,
        ) {
            // fallbacks
            if is_player {
                p_slide_move(level, pos, vel, class, &slide_normal);
            } else if flags.0.contains(MobjFlags::MISSILE) {
                acts.push(Action::Explode { entity: ent });
            } else {
                vel.0.x = 0.0;
                vel.0.y = 0.0;
            }
        }
    }

    /* -- 3: friction / stop ---------------------------------------- */
    if !flags.0.intersects(MobjFlags::MISSILE | MobjFlags::SKULLFLY)
        && pos.1 <= get_floor_z(level, subsector)
    {
        if vel.0.x.abs() < STOP_SPEED && vel.0.y.abs() < STOP_SPEED && player_cmd_idle() {
            if is_player && (anim.state >= State::PLAY_RUN1 && anim.state <= State::PLAY_RUN4) {
                acts.push(Action::SetState {
                    entity: ent,
                    new_state: State::PLAY,
                });
            }
            vel.0.x = 0.0;
            vel.0.y = 0.0;
        } else {
            vel.0.x *= FRICTION;
            vel.0.y *= FRICTION;
        }
    }

    acts
}

/* ================================================================= */
/*  Helpers – still many TODOs                                       */
/* ================================================================= */

fn player_cmd_idle() -> bool {
    false /* TODO */
}

fn get_floor_z(level: &Level, sub: &Subsector) -> f32 {
    level.sectors[level.subsectors[sub.0 as usize].sector as usize].floor_h
}

#[allow(clippy::too_many_arguments)]
fn p_try_move(
    level: &Level,
    pos: &mut Pos,
    sub: &mut Subsector,
    class: &Class,
    is_player: bool,
    dest: Vec2,
    slide_nrm: &mut Option<Vec2>,
) -> bool {
    let check = p_check_position(level, class, is_player, dest);

    if check.blocked
        || check.ceiling_z - check.floor_z < class.0.height as f32
        || check.floor_z - pos.1 > MAX_STEP_HEIGHT
        || check.floor_z - check.dropoff_z > MAX_STEP_HEIGHT
    {
        *slide_nrm = None; // TODO
        return false;
    }

    p_cross_special_lines(level, dest, pos.0, check.special_lines);

    // relink
    p_unset_thing_position(pos, sub);
    pos.0 = dest;
    pos.1 = check.floor_z;
    sub.0 = check.subsector;
    p_set_thing_position(pos, sub);

    true
}

fn box_on_line_side(b: &Aabb, v1: Vec2, v2: Vec2) -> i32 {
    let dx = v2.x - v1.x;
    let dy = v2.y - v1.y;
    let mut front = false;
    let mut back = false;

    for &x in &[b.min.x, b.max.x] {
        for &y in &[b.min.y, b.max.y] {
            let cross = dx * (y - v1.y) - (x - v1.x) * dy;
            if cross >= 0.0 {
                front = true
            } else {
                back = true
            }
            if front && back {
                return -1;
            }
        }
    }
    if front { 0 } else { 1 } // 0 = front, 1 = back  (vanilla)
}

#[inline]
pub fn line_opening(level: &Level, line: &Linedef) -> (f32, f32, f32, f32) {
    // if either side is missing → single-sided wall
    let (front_sd, back_sd) = match (line.right_sidedef, line.left_sidedef) {
        (Some(f), Some(b)) => (f as usize, b as usize),
        _ => return (0.0, 0.0, 0.0, 0.0), // open_range = 0  → blocked
    };

    let front_sec = &level.sectors[level.sidedefs[front_sd].sector as usize];
    let back_sec = &level.sectors[level.sidedefs[back_sd].sector as usize];

    let open_top = front_sec.ceil_h.min(back_sec.ceil_h); // lower ceiling
    let (open_bottom, low_floor) = if front_sec.floor_h > back_sec.floor_h {
        (front_sec.floor_h, back_sec.floor_h)
    } else {
        (back_sec.floor_h, front_sec.floor_h)
    };

    let open_range = open_top - open_bottom; // may be ≤ 0 if closed

    (open_top, open_bottom, open_range, low_floor)
}

#[derive(Default)]
pub struct CheckCtx {
    pub bbox: Aabb,
    pub floor_z: f32,
    pub ceiling_z: f32,
    pub dropoff_z: f32,
    pub ceilingline: Option<LinedefId>,
    pub thing_is_missile: bool,
    pub thins_is_player: bool,
    pub special_lines: SmallVec<[LinedefId; 4]>,
}

/// returns *false* when the line blocks the move
pub fn pit_check_line(level: &Level, line: &Linedef, ctx: &mut CheckCtx) -> bool {
    /* fast AABB reject ---------------------------------------------- */
    if ctx.bbox.max.x <= line.bbox.min.x
        || ctx.bbox.min.x >= line.bbox.max.x
        || ctx.bbox.max.y <= line.bbox.min.y
        || ctx.bbox.min.y >= line.bbox.max.y
    {
        return true;
    }

    /* all corners on same side ? ------------------------------------ */
    let v1 = level.vertices[line.v1 as usize].pos;
    let v2 = level.vertices[line.v2 as usize].pos;
    if box_on_line_side(&ctx.bbox, v1, v2) != -1 {
        return true;
    }

    /* solid / monster-only blocking --------------------------------- */
    if !line.flags.contains(LinedefFlags::TWO_SIDED) {
        return false; // one-sided wall
    }
    if !ctx.thing_is_missile {
        if line.flags.contains(LinedefFlags::IMPASSABLE) {
            return false;
        }
        if !ctx.thins_is_player && line.flags.contains(LinedefFlags::BLOCK_MONSTERS) {
            return false;
        }
    }

    /* opening & height adjustments ---------------------------------- */
    let (open_top, open_bottom, _, low_floor) = line_opening(level, line);

    if open_top < ctx.ceiling_z {
        ctx.ceiling_z = open_top;
        ctx.ceilingline = Some(line.id);
    }
    if open_bottom > ctx.floor_z {
        ctx.floor_z = open_bottom;
    }
    if low_floor < ctx.dropoff_z {
        ctx.dropoff_z = low_floor;
    }

    /* remember specials --------------------------------------------- */
    if line.special != 0 {
        ctx.special_lines.push(line.id);
    }
    true
}

/// Everything P_CheckPosition discovered for the tentative spot.
pub struct CheckResult {
    pub blocked: bool,
    pub floor_z: f32,
    pub ceiling_z: f32,
    pub dropoff_z: f32,
    pub subsector: u16,
    pub special_lines: SmallVec<[LinedefId; 4]>,
}

/// Full collision test (lines + things) at <dest>.
/// *Return `None` for a solid block; otherwise return floor/ceiling data.*
fn p_check_position(level: &Level, class: &Class, is_player: bool, dest: Vec2) -> CheckResult {
    let radius = class.0.radius as f32;

    /* locate subsector & initialise floor / ceiling */
    let ss_idx = level.locate_subsector(dest);
    let ssd = &level.subsectors[ss_idx as usize];
    let sector = &level.sectors[ssd.sector as usize];

    /* bounding box the actor’s cylinder occupies */
    let bbox = Aabb {
        min: dest - Vec2::splat(radius),
        max: dest + Vec2::splat(radius),
    };

    let mut ctx = CheckCtx {
        bbox,
        floor_z: sector.floor_h,
        ceiling_z: sector.ceil_h,
        dropoff_z: sector.floor_h,
        ceilingline: None,
        thing_is_missile: class.0.flags.contains(MobjFlags::MISSILE),
        thins_is_player: is_player,
        special_lines: SmallVec::<[LinedefId; 4]>::new(),
    };

    let blocked = !level.block_lines_iter(bbox, |ld| pit_check_line(level, ld, &mut ctx));

    CheckResult {
        blocked,
        floor_z: ctx.floor_z,
        ceiling_z: ctx.ceiling_z,
        dropoff_z: ctx.dropoff_z,
        subsector: ss_idx,
        special_lines: ctx.special_lines,
    }
}

/*================================================================ */
/* ===  Small helper *stubs*  ==================================== */
/*================================================================ */

/// Remove the actor from the spatial data-structures (blockmap / BSP).
fn p_unset_thing_position(_pos: &Pos, _sub: &Subsector) {
    /* TODO */
}

/// Re-link the actor at its new coordinates.
fn p_set_thing_position(_pos: &Pos, _sub: &Subsector) {
    /* TODO */
}

/// Check special lines crossed between <old_xy> → <new_xy>.
fn p_cross_special_lines(
    _level: &Level,
    _new_xy: Vec2,
    _old_xy: Vec2,
    _special_lines: SmallVec<[LinedefId; 4]>,
) {
    /* TODO (use P_PointOnLineSide + P_CrossSpecialLine later) */
}

/*----------------- helper stubs to fill later -----------------*/
fn p_slide_move(
    _level: &Level,
    _pos: &mut Pos,
    _vel: &mut Vel,
    _class: &Class,
    _slide_nrm: &Option<Vec2>,
) {
    // todo!("faithful Doom P_SlideMove")
}

fn p_set_mobj_state(world: &mut World, entity: Entity, new_state: State) {
    if let Ok(mut anim) = world.get::<&mut Anim>(entity) {
        anim.state = new_state;
    }
    /* TODO */
}

fn p_explode_missile(world: &mut World, entity: Entity, _level: &Level) {
    world.despawn(entity).ok();
    /* TODO missile impact FX */
}
