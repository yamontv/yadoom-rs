use crate::{
    renderer::software::Software,
    world::{
        camera::Camera,
        geometry::{Level, SegmentId, SubsectorId},
        texture::{NO_TEXTURE, TextureBank, TextureId},
    },
};
use bitflags::bitflags;

#[derive(Clone, Default)]
pub struct DrawSeg {
    pub cur_line: SegmentId,
    pub x1: i32,
    pub x2: i32,

    pub scale1: f32,
    pub scale2: f32,
    pub scale_step: f32,

    pub silhouette: Silhouette,
    pub bsil_height: f32, // do not clip sprites above this
    pub tsil_height: f32, // do not clip sprites below this

    pub masked_mid: Option<TextureId>,
}

bitflags! {
    #[derive(Default, Clone)]
    pub struct Silhouette: u8 {
        const NONE = 0x0000;
        const BOTTOM = 0x0001;
        const TOP    = 0x0002;
        const SOLID  = 0x0003;
    }
}

#[derive(Clone, Copy)]
pub struct VisSprite {
    pub x0: i32, // inclusive
    pub x1: i32, // inclusive
    pub y0: i32,
    pub y1: i32,
    pub invz: f32, // 1 / camera-space Y (depth)
    pub tex: TextureId,
    pub u_step: f32, // how far to advance U per screen pixel X
}

/// very small subset just to get moving
const THING_SPRITE: &[(u16, &str)] = &[
    (1, "PLAYA1"),    // player 1 start
    (2014, "BON1A0"), // BON
    (3001, "TROOA0"), // imp (front-facing, no rotation)
    (3004, "POSSA0"), // zombieman
    (2004, "CLIPA0"), // clip pickup
];

fn sprite_for(type_id: u16, tex_bank: &TextureBank) -> TextureId {
    THING_SPRITE
        .iter()
        .find(|(id, _)| *id == type_id)
        .and_then(|(_, lump)| tex_bank.id(*lump))
        .unwrap_or(NO_TEXTURE)
}

impl Software {
    // ---------------------------------------------------------------------
    // Phase 1: build a list of sprites that enter the view
    // ---------------------------------------------------------------------
    pub fn collect_sprites_for_subsector(
        &mut self,
        ss_idx: SubsectorId,
        level: &Level,
        camera: &Camera,
        tex_bank: &TextureBank,
    ) {
        let mut out: Vec<VisSprite> = Vec::new();
        let focal = camera.screen_scale(self.width);
        let half_w = self.half_w;
        let half_h = self.half_h;

        let sec_idx = level.subsectors[ss_idx as usize].sector;
        let floor_z = level.sectors[sec_idx as usize].floor_h as f32;

        for thing_idx in level.subsectors[ss_idx as usize].things.iter() {
            let thing = &level.things[*thing_idx as usize];

            let tex_id = sprite_for(thing.type_id, tex_bank);

            if tex_id == NO_TEXTURE {
                continue;
            }

            // camera space -------------------------------------------------
            let rel = camera.to_cam(&thing.pos); // z=0 floor aligned
            if rel.y <= 4.0 {
                // “behind” or too close to near-plane
                continue;
            }
            let invz = 1.0 / rel.y;
            let scale = focal * invz;

            let tex = tex_bank.texture(tex_id).unwrap();
            let sprite_w = tex.w as f32 * scale;
            let sprite_h = tex.h as f32 * scale;

            let xc = half_w + rel.x * scale;
            let x0 = (xc - sprite_w * 0.5).floor() as i32;
            let x1 = (xc + sprite_w * 0.5).ceil() as i32;

            if x1 < 0 || x0 >= self.width as i32 {
                continue; // completely off-screen
            }

            // vertical offset between sprite base (sector floor) and the eye
            let rel_z = floor_z - camera.pos.z;

            let y_bottom = half_h - rel_z * scale;

            let y0 = (y_bottom - sprite_h).floor() as i32; // top
            let y1 = (y_bottom).ceil() as i32; // bottom (touching floor)

            out.push(VisSprite {
                x0,
                x1,
                y0,
                y1,
                invz,
                tex: tex_id,
                u_step: tex.w as f32 / (x1 - x0 + 1) as f32,
            });
        }

        // far-to-near painter’s algorithm so we overdraw correctly
        out.sort_by(|a, b| b.invz.partial_cmp(&a.invz).unwrap());
        self.sprites.append(&mut out);
    }

    // ---------------------------------------------------------------------
    // Phase 2: draw them, column by column
    // ---------------------------------------------------------------------
    pub fn draw_sprites(&mut self, tex_bank: &TextureBank) {
        for spr in &self.sprites {
            let tex = tex_bank.texture(spr.tex).unwrap();
            let mut u_f = 0.0;
            let mut x = spr.x0.max(0);
            let x_end = spr.x1.min(self.width as i32 - 1);
            let u_step = spr.u_step;

            // column loop --------------------------------------------------
            while x <= x_end {
                // occlusion test – skip columns fully hidden by solid walls
                if self.is_column_occluded(x) {
                    u_f += u_step;
                    x += 1;
                    continue;
                }

                let u = u_f as usize;
                if u >= tex.w {
                    break;
                }

                // clip top/bottom against visplane bands
                let ceil = self.clip_bands.ceil[x as usize] as i32;
                let floor = self.clip_bands.floor[x as usize] as i32;
                let y0 = spr.y0.max(ceil).max(0);
                let y1 = spr.y1.min(floor).min(self.height as i32 - 1);

                // draw one vertical slice ---------------------------------
                let v_step = tex.h as f32 / (spr.y1 - spr.y0 + 1) as f32;
                let mut v_f = (y0 - spr.y0) as f32 * v_step;
                for y in y0..=y1 {
                    let v = v_f as usize;
                    let idx = tex.pixels[v * tex.w + u];
                    if idx != 0 {
                        // 0 = fully transparent
                        self.scratch[y as usize * self.width + x as usize] =
                            tex_bank.get_color(0, idx);
                    }
                    v_f += v_step;
                }

                u_f += u_step;
                x += 1;
            }
        }
    }

    #[inline]
    fn is_column_occluded(&self, x: i32) -> bool {
        for seg in &self.solid_segs {
            if x >= seg.first && x <= seg.last {
                return true;
            }
        }
        false
    }
}
