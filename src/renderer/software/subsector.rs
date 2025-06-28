use crate::{
    renderer::software::{
        Software,
        planes::{NO_PLANE, VisplaneId},
        projection::Edge,
        sprites::Silhouette,
    },
    world::{
        geometry::{Level, Linedef, LinedefFlags, Sector, Seg, SegmentId, Sidedef},
        texture::{NO_TEXTURE, Texture, TextureBank, TextureId},
    },
};

#[derive(Clone, Copy, PartialEq)]
enum ClipKind {
    Solid,
    Upper,
    Lower,
}

#[derive(Clone, Debug)]
pub struct WallSpan {
    pub tex_id: TextureId,
    pub shade_idx: u8,

    /* perspective-correct texture coords (already divided by z) */
    pub u0_over_z: f32,
    pub u1_over_z: f32,
    pub inv_z0: f32,
    pub inv_z1: f32,

    /* screen extents */
    pub x_start: i32,
    pub x_end: i32,
    pub y_top0: f32, // ceiling at x_start
    pub y_top1: f32, // ceiling at x_end
    pub y_bot0: f32, // floor   at x_start
    pub y_bot1: f32, // floor   at x_end

    pub wall_h: f32,        // ceiling_z - floor_z in map units
    pub texturemid_mu: f32, // (ceil_h − eyeZ) + y_off     in map units
}

/// Per‑column attributes advance linearly across the span.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Step {
    duoz: f32,
    dinvz: f32,
    dytop: f32,
    dybot: f32,
}
impl Step {
    #[inline]
    fn from_span(s: &WallSpan) -> Self {
        let w = (s.x_end - s.x_start).max(1) as f32;
        Self {
            duoz: (s.u1_over_z - s.u0_over_z) / w,
            dinvz: (s.inv_z1 - s.inv_z0) / w,
            dytop: (s.y_top1 - s.y_top0) / w,
            dybot: (s.y_bot1 - s.y_bot0) / w,
        }
    }
}

/// Per‑column cursor that marches from left → right.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Cursor {
    uoz: f32,
    inv_z: f32,
    y_top: f32,
    y_bot: f32,
}
impl Cursor {
    #[inline]
    fn from_span(s: &WallSpan) -> Self {
        Self {
            uoz: s.u0_over_z,
            inv_z: s.inv_z0,
            y_top: s.y_top0,
            y_bot: s.y_bot0,
        }
    }

    #[inline(always)]
    fn advance(&mut self, s: &Step) {
        self.uoz += s.duoz;
        self.inv_z += s.dinvz;
        self.y_top += s.dytop;
        self.y_bot += s.dybot;
    }
}

enum WallPass {
    Solid {
        pegged: bool,
        world_top: f32,
        world_bottom: f32,
        middle_texture: TextureId,
    },
    TwoSided {
        pegged: bool,
        world_top: f32,
        world_bottom: f32,
        mark_floor: bool,
        mark_ceiling: bool,
        upper_floor_h: f32,
        upper_tex: TextureId,
        lower_ceil_h: f32,
        lower_tex: TextureId,
    },
}

impl Software {
    fn sectors_for_seg<'l>(
        &self,
        seg: &Seg,
        level: &'l Level,
    ) -> (&'l Sidedef, Option<&'l Sector>, &'l Linedef) {
        let ld = &level.linedefs[seg.linedef as usize];
        let (sd_front_idx, sd_back_idx) = if seg.dir == 0 {
            (ld.right_sidedef, ld.left_sidedef)
        } else {
            (ld.left_sidedef, ld.right_sidedef)
        };
        let front = &level.sidedefs[sd_front_idx.unwrap() as usize];
        let back = sd_back_idx
            .and_then(|i| level.sidedefs.get(i as usize))
            .map(|sd| &level.sectors[sd.sector as usize]);
        (front, back, ld)
    }

    pub fn draw_edge(
        &mut self,
        edge: Edge,
        seg_idx: SegmentId,
        level: &Level,
        texture_bank: &TextureBank,
    ) {
        let seg = &level.segs[seg_idx as usize];
        let (sd_front, sec_back_opt, ld) = self.sectors_for_seg(seg, level);
        let sec_front = &level.sectors[sd_front.sector as usize];

        let light = (sec_front.light * 255.0) as i16;

        let floor_vis = if sec_front.floor_h < self.view_z {
            self.visplane_map.find(
                sec_front.floor_h as i16,
                sec_front.floor_tex,
                light,
                edge.x_l as u16,
                edge.x_r as u16,
            )
        } else {
            NO_PLANE
        };

        let ceil_vis = if sec_front.ceil_h > self.view_z {
            self.visplane_map.find(
                sec_front.ceil_h as i16,
                sec_front.ceil_tex,
                light,
                edge.x_l as u16,
                edge.x_r as u16,
            )
        } else {
            NO_PLANE
        };

        let mut ds = self.create_draw_seg(
            seg_idx,
            &edge,
            if sec_back_opt.is_some() {
                sd_front.middle
            } else {
                NO_TEXTURE
            },
        );

        let pass = self.decide_pass(sec_front, sec_back_opt, sd_front, ld);

        match pass {
            WallPass::Solid {
                pegged,
                world_top,
                world_bottom,
                middle_texture,
            } => {
                self.push_wall(
                    &edge,
                    world_top,
                    world_bottom,
                    sec_front.light,
                    middle_texture,
                    ClipKind::Solid,
                    pegged,
                    sd_front.y_off,
                    ceil_vis,
                    floor_vis,
                    texture_bank,
                );
                self.add_solid_seg(edge.x_l, edge.x_r);
                ds.silhouette = Silhouette::SOLID;
            }
            WallPass::TwoSided {
                pegged,
                world_top,
                world_bottom,
                mark_floor,
                mark_ceiling,
                upper_floor_h,
                upper_tex,
                lower_ceil_h,
                lower_tex,
            } => {
                let cur_floor_vis = if mark_floor { floor_vis } else { NO_PLANE };
                let cur_ceil_vis = if mark_ceiling { ceil_vis } else { NO_PLANE };
                self.push_wall(
                    &edge,
                    world_top,
                    upper_floor_h,
                    sec_front.light,
                    upper_tex,
                    ClipKind::Upper,
                    pegged,
                    sd_front.y_off,
                    cur_ceil_vis,
                    NO_PLANE,
                    texture_bank,
                );

                self.push_wall(
                    &edge,
                    lower_ceil_h,
                    world_bottom,
                    sec_front.light,
                    lower_tex,
                    ClipKind::Lower,
                    pegged,
                    sd_front.y_off,
                    NO_PLANE,
                    cur_floor_vis,
                    texture_bank,
                );

                if upper_floor_h > world_bottom {
                    ds.silhouette.insert(Silhouette::BOTTOM);
                    ds.bsil_height = upper_floor_h; // world Z, not screen Y
                }

                if lower_ceil_h < world_top {
                    ds.silhouette.insert(Silhouette::TOP);
                    ds.tsil_height = lower_ceil_h; // world Z
                }
            }
        }

        self.update_draw_seg_clips(&mut ds);

        self.drawsegs.push(ds);
    }

    fn decide_pass(
        &self,
        sec_front: &Sector,
        sec_back_opt: Option<&Sector>,
        sd_front: &Sidedef,
        ld: &Linedef,
    ) -> WallPass {
        let world_top = sec_front.ceil_h;
        let world_bottom = sec_front.floor_h;

        if sec_back_opt.is_some() && ld.flags.contains(LinedefFlags::TWO_SIDED) {
            let sec_back = sec_back_opt.unwrap();
            let worldhigh = sec_back.ceil_h;
            let worldlow = sec_back.floor_h;

            let mut mark_floor;
            let mut mark_ceiling;

            if worldlow != world_bottom
                || sec_back.floor_tex != sec_front.floor_tex
                || sec_back.light != sec_front.light
            {
                // not the same plane on both sides
                mark_floor = true;
            } else {
                // same plane on both sides
                mark_floor = false;
            }

            if worldhigh != world_top
                || sec_back.ceil_tex != sec_front.ceil_tex
                || sec_back.light != sec_front.light
            {
                mark_ceiling = true;
            } else {
                // same plane on both sides
                mark_ceiling = false;
            }

            if worldhigh <= world_bottom || worldlow >= world_top {
                // closed door
                mark_ceiling = true;
                mark_floor = true;
            }

            // ─ upper portal
            let upper_floor_h = worldhigh.min(world_top);
            let upper_tex = if worldhigh < world_top {
                sd_front.upper
            } else {
                NO_TEXTURE
            };

            // ─ lower portal
            let lower_ceil_h = worldlow.max(world_bottom);
            let lower_tex = if worldlow > world_bottom {
                sd_front.lower
            } else {
                NO_TEXTURE
            };
            WallPass::TwoSided {
                pegged: ld.flags.contains(LinedefFlags::UPPER_UNPEGGED),
                world_top,
                world_bottom,
                mark_floor,
                mark_ceiling,
                upper_floor_h,
                upper_tex,
                lower_ceil_h,
                lower_tex,
            }
        } else {
            WallPass::Solid {
                pegged: ld.flags.contains(LinedefFlags::LOWER_UNPEGGED),
                world_top,
                world_bottom,
                middle_texture: sd_front.middle,
            }
        }
    }

    fn push_wall(
        &mut self,
        edge: &Edge,
        ceil_h: f32,
        floor_h: f32,
        light: f32,
        tex: TextureId,
        kind: ClipKind,
        pegged: bool,
        y_off: f32,
        ceil_vis: VisplaneId,
        floor_vis: VisplaneId,
        texture_bank: &TextureBank,
    ) {
        let texturemid_mu = match (kind, pegged) {
            (ClipKind::Lower, true) => (ceil_h - self.view_z) + y_off,
            (ClipKind::Lower, false) => (floor_h - self.view_z) + y_off,
            // everything else (Solid + Upper):
            (_, true) => (floor_h - self.view_z) + y_off,
            (_, false) => (ceil_h - self.view_z) + y_off,
        };

        self.emit_and_clip(
            &WallSpan {
                /* projection */
                tex_id: tex,
                shade_idx: ((1.0 - light) * 31.0) as u8,
                u0_over_z: edge.uoz_l,
                u1_over_z: edge.uoz_r,
                inv_z0: edge.invz_l,
                inv_z1: edge.invz_r,
                x_start: edge.x_l,
                x_end: edge.x_r,
                y_top0: self.half_h - (ceil_h - self.view_z) * self.focal * edge.invz_l,
                y_top1: self.half_h - (ceil_h - self.view_z) * self.focal * edge.invz_r,
                y_bot0: self.half_h - (floor_h - self.view_z) * self.focal * edge.invz_l,
                y_bot1: self.half_h - (floor_h - self.view_z) * self.focal * edge.invz_r,
                /* tiling */
                wall_h: (ceil_h - floor_h).abs(),
                texturemid_mu,
            },
            kind,
            ceil_vis,
            floor_vis,
            texture_bank,
        );
    }

    #[inline]
    fn column_visible(&self, col: usize, y_top: f32, y_bot: f32) -> bool {
        y_top < self.clip_bands.floor[col] as f32 && y_bot > self.clip_bands.ceil[col] as f32
    }

    #[inline]
    fn draw_column(
        &mut self,
        col: usize,
        cur: &Cursor,
        span: &WallSpan,
        bank: &TextureBank,
        tex: &Texture,
        y_min: i16,
        y_max: i16,
    ) {
        if y_max < y_min {
            return;
        }

        // Fixed DOOM vertical scaling.
        let col_px_h = (cur.y_bot - cur.y_top).max(1.0);
        let dv_mu = span.wall_h / col_px_h; // map‑units per pixel
        let mut v_mu = span.texturemid_mu + (y_min as f32 - self.half_h) * dv_mu;

        // Horizontal texture coordinate stays constant in a column.
        let u_tex = ((cur.uoz / cur.inv_z) as i32).rem_euclid(tex.w as i32) as usize;

        for y in y_min..=y_max {
            let v_tex = (v_mu as i32).rem_euclid(tex.h as i32) as usize;
            self.scratch[y as usize * self.width + col] =
                bank.get_color(span.shade_idx, tex.pixels[v_tex * tex.w + u_tex]);
            v_mu += dv_mu;
        }
    }

    fn emit_and_clip(
        &mut self,
        proto: &WallSpan,
        kind: ClipKind,
        ceil_vis: VisplaneId,
        floor_vis: VisplaneId,
        texture_bank: &TextureBank,
    ) {
        let step = Step::from_span(proto);
        let mut cur = Cursor::from_span(proto);

        let tex = texture_bank
            .texture(proto.tex_id)
            .unwrap_or_else(|_| texture_bank.texture(NO_TEXTURE).unwrap());

        for x in proto.x_start..=proto.x_end {
            let col = x as usize;

            if self.clip_bands.ceil[col] < self.clip_bands.floor[col] {
                // part of the wall that was visible in this column
                let y0 = cur.y_top.max((self.clip_bands.ceil[col] + 1) as f32).ceil() as i16;
                let y1 = cur
                    .y_bot
                    .min((self.clip_bands.floor[col] - 1) as f32)
                    .floor() as i16;

                if proto.tex_id != NO_TEXTURE && self.column_visible(col, cur.y_top, cur.y_bot) {
                    self.draw_column(
                        col,
                        &cur,
                        proto,
                        texture_bank,
                        tex,
                        y0.max(0),
                        y1.min((self.height - 1) as i16),
                    );
                }

                if let Some(vp) = self.visplane_map.get(ceil_vis) {
                    let top = self.clip_bands.ceil[col] + 1;
                    let bottom = (y0 - 1).min(self.clip_bands.floor[col] - 1);

                    if top <= bottom {
                        vp.modified = true;
                        vp.top[col] = top.max(0) as u16;
                        vp.bottom[col] = bottom.max(0) as u16;
                    }
                }

                if let Some(vp) = self.visplane_map.get(floor_vis) {
                    let top = (y1 + 1).max(self.clip_bands.ceil[col]);
                    let bottom = self.clip_bands.floor[col];
                    if top <= bottom {
                        vp.modified = true;
                        vp.top[col] = top.max(0) as u16;
                        vp.bottom[col] = bottom.max(0) as u16;
                    }
                }

                match kind {
                    ClipKind::Solid => {
                        self.clip_bands.ceil[col] = i16::MAX;
                        self.clip_bands.floor[col] = i16::MIN;
                    }
                    ClipKind::Upper => {
                        if proto.tex_id != NO_TEXTURE || ceil_vis != NO_PLANE {
                            self.clip_bands.ceil[col] = self.clip_bands.ceil[col].max(y1 + 1);
                        }
                    }
                    ClipKind::Lower => {
                        if proto.tex_id != NO_TEXTURE || floor_vis != NO_PLANE {
                            self.clip_bands.floor[col] = self.clip_bands.floor[col].min(y0 - 1);
                        }
                    }
                }
            }

            cur.advance(&step);
        }
    }
}
