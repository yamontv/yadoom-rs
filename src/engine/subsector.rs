use crate::{
    engine::engine::Engine,
    engine::planes::{NO_PLANE, VisplaneId},
    engine::types::{ClipRange, Edge},
    renderer::{Renderer, Rgba, WallSpan},
    world::{
        geometry::{Linedef, LinedefFlags, Sector, Seg, Sidedef},
        texture::{NO_TEXTURE, TextureId},
    },
};

#[derive(Clone, Copy, PartialEq)]
enum ClipKind {
    Solid,
    Upper,
    Lower,
}

enum WallPass {
    Solid {
        pegged: bool,
        world_top: i16,
        world_bottom: i16,
    },
    TwoSided {
        pegged: bool,
        world_top: i16,
        world_bottom: i16,
        mark_floor: bool,
        mark_ceiling: bool,
        upper_floor_h: i16,
        upper_tex: TextureId,
        lower_ceil_h: i16,
        lower_tex: TextureId,
    },
}

impl<R: Renderer> Engine<R> {
    pub fn draw_subsector(&mut self, ss_idx: u16, _submit: &mut impl FnMut(&[Rgba], usize, usize)) {
        let ss = &self.level.subsectors[ss_idx as usize];
        let start = ss.first_seg;
        let end = start + ss.seg_count;

        for seg_idx in start..end {
            // Back‑face cull in *world* space: if the viewer is on the back side
            // of the SEG’s plane, skip it.
            if self.back_facing_seg(seg_idx) {
                continue;
            }

            if let Some(edge) = self.project_seg(seg_idx) {
                self.build_spans(&edge);

                // self.visplane_map.draw_all(
                //     &mut self.renderer,
                //     &self.level,
                //     &self.camera,
                //     &self.screen,
                //     &self.view,
                //     &self.texture_bank,
                // );
                // self.debug_draw_solid_segs();
                // self.renderer.end_frame(|pixels, ww, hh| {
                //     _submit(pixels, ww, hh);
                // });
            }
        }
    }

    fn sectors_for_seg(&self, seg: &Seg) -> (&Sidedef, Option<&Sector>, &Linedef) {
        let ld = &self.level.linedefs[seg.linedef as usize];
        let (sd_front_idx, sd_back_idx) = if seg.dir == 0 {
            (ld.right_sidedef, ld.left_sidedef)
        } else {
            (ld.left_sidedef, ld.right_sidedef)
        };
        let front = &self.level.sidedefs[sd_front_idx.unwrap() as usize];
        let back = sd_back_idx
            .and_then(|i| self.level.sidedefs.get(i as usize))
            .map(|sd| &self.level.sectors[sd.sector as usize]);
        (front, back, ld)
    }

    fn decide_pass(
        &self,
        sd_front: &Sidedef,
        sec_front: &Sector,
        sec_back_opt: Option<&Sector>,
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
            }
        }
    }

    fn build_spans(&mut self, edge: &Edge) {
        let seg = &self.level.segs[edge.seg_idx as usize];
        let (sd_front, sec_back_opt, ld) = self.sectors_for_seg(seg);
        let sec_front = &self.level.sectors[sd_front.sector as usize];

        let pass = self.decide_pass(sd_front, sec_front, sec_back_opt, ld);

        let sd_y_off = sd_front.y_off as f32;
        let light = sec_front.light;
        let middle_tex = sd_front.middle;

        let floor_vis = if (sec_front.floor_h as f32) < self.view.view_z {
            self.visplane_map.find(
                sec_front.floor_h,
                sec_front.floor_tex,
                sec_front.light,
                edge.x_l.max(0) as u16,
                edge.x_r.max(0) as u16,
            )
        } else {
            NO_PLANE
        };

        let ceil_vis = if (sec_front.ceil_h as f32) > self.view.view_z {
            self.visplane_map.find(
                sec_front.ceil_h,
                sec_front.ceil_tex,
                sec_front.light,
                edge.x_l.max(0) as u16,
                edge.x_r.max(0) as u16,
            )
        } else {
            NO_PLANE
        };

        match pass {
            WallPass::Solid {
                pegged,
                world_top,
                world_bottom,
            } => {
                self.push_wall(
                    edge,
                    world_top as f32,
                    world_bottom as f32,
                    light,
                    middle_tex,
                    ClipKind::Solid,
                    pegged,
                    sd_y_off,
                    ceil_vis,
                    floor_vis,
                );
                self.add_solid_seg(edge.x_l, edge.x_r);
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
                    edge,
                    world_top as f32,
                    upper_floor_h as f32,
                    light,
                    upper_tex,
                    ClipKind::Upper,
                    pegged,
                    sd_y_off,
                    cur_ceil_vis,
                    NO_PLANE,
                );

                self.push_wall(
                    edge,
                    lower_ceil_h as f32,
                    world_bottom as f32,
                    light,
                    lower_tex,
                    ClipKind::Lower,
                    pegged,
                    sd_y_off,
                    NO_PLANE,
                    cur_floor_vis,
                );
            }
        }
    }

    fn push_wall(
        &mut self,
        edge: &Edge,
        ceil_h: f32,
        floor_h: f32,
        light: i16,
        tex: TextureId,
        kind: ClipKind,
        pegged: bool,
        y_off: f32,
        ceil_vis: VisplaneId,
        floor_vis: VisplaneId,
    ) {
        let texturemid_mu = match (kind, pegged) {
            (ClipKind::Lower, true) => (ceil_h - self.view.view_z) + y_off,
            (ClipKind::Lower, false) => (floor_h - self.view.view_z) + y_off,
            // everything else (Solid + Upper):
            (_, true) => (floor_h - self.view.view_z) + y_off,
            (_, false) => (ceil_h - self.view.view_z) + y_off,
        };

        self.emit_and_clip(
            &WallSpan {
                /* projection */
                tex_id: tex,
                light,
                u0_over_z: edge.uoz_l,
                u1_over_z: edge.uoz_r,
                inv_z0: edge.invz_l,
                inv_z1: edge.invz_r,
                x_start: edge.x_l,
                x_end: edge.x_r,
                y_top0: self.screen.half_h
                    - (ceil_h - self.view.view_z) * self.view.focal * edge.invz_l,
                y_top1: self.screen.half_h
                    - (ceil_h - self.view.view_z) * self.view.focal * edge.invz_r,
                y_bot0: self.screen.half_h
                    - (floor_h - self.view.view_z) * self.view.focal * edge.invz_l,
                y_bot1: self.screen.half_h
                    - (floor_h - self.view.view_z) * self.view.focal * edge.invz_r,
                /* tiling */
                wall_h: (ceil_h - floor_h).abs(),
                texturemid_mu,
            },
            kind,
            ceil_vis,
            floor_vis,
        );
    }

    fn emit_and_clip(
        &mut self,
        proto: &WallSpan,
        kind: ClipKind,
        ceil_vis: VisplaneId,
        floor_vis: VisplaneId,
    ) {
        // draw first, while bands still contain the old limits
        if proto.tex_id != NO_TEXTURE {
            self.renderer
                .draw_wall(proto, &self.clip_bands, &self.texture_bank);
        }

        // now update bands for every column that was really drawn
        let w = (proto.x_end - proto.x_start).max(1) as f32;
        let dyt = (proto.y_top1 - proto.y_top0) / w;
        let dyb = (proto.y_bot1 - proto.y_bot0) / w;
        let mut y_t = proto.y_top0;
        let mut y_b = proto.y_bot0;

        for x in proto.x_start..=proto.x_end {
            let col = x as usize;

            if self.clip_bands.ceil[col] < self.clip_bands.floor[col] {
                // part of the wall that was visible in this column
                let y0 = y_t.max((self.clip_bands.ceil[col] + 1) as f32).ceil() as i16;
                let y1 = y_b.min((self.clip_bands.floor[col] - 1) as f32).floor() as i16;

                if let Some(vp) = self.visplane_map.get(ceil_vis) {
                    let top = self.clip_bands.ceil[col] + 1;
                    let bottom = (y0 - 1).min(self.clip_bands.floor[col] - 1);

                    if top <= bottom {
                        vp.top[col] = top.max(0) as u16;
                        vp.bottom[col] = bottom.max(0) as u16;
                    }
                }

                if let Some(vp) = self.visplane_map.get(floor_vis) {
                    let top = (y1 + 1).max(self.clip_bands.ceil[col]);
                    let bottom = self.clip_bands.floor[col];
                    if top <= bottom {
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

            y_t += dyt;
            y_b += dyb;
        }
    }

    pub fn init_solid_segs(&mut self) {
        let w = self.screen.w as i32;
        self.solid_segs.clear();
        // Two sentinels so our add routine never has to worry
        // about running off the ends of the array.
        self.solid_segs.push(ClipRange {
            first: -w,
            last: -1,
        });
        self.solid_segs.push(ClipRange {
            first: w,
            last: w * 2,
        });
    }

    pub fn add_solid_seg(&mut self, first: i32, last: i32) {
        let mut i = 0;
        // 1) skip all segments that end *before* ours minus one
        while i < self.solid_segs.len() && self.solid_segs[i].last < first - 1 {
            i += 1;
        }

        // if the new segment is completely swallowed by an existing one,
        // we’re done early
        if i < self.solid_segs.len()
            && first >= self.solid_segs[i].first
            && last <= self.solid_segs[i].last
        {
            return;
        }

        // 2) merge any overlapping or adjacent segments:
        let mut new_first = first;
        let mut new_last = last;
        while i < self.solid_segs.len() && self.solid_segs[i].first <= last + 1 {
            new_first = new_first.min(self.solid_segs[i].first);
            new_last = new_last.max(self.solid_segs[i].last);
            self.solid_segs.remove(i);
        }

        // 3) insert the coalesced segment in its sorted place
        self.solid_segs.insert(
            i,
            ClipRange {
                first: new_first,
                last: new_last,
            },
        );
    }

    pub fn debug_draw_solid_segs(&mut self) {
        // Compute the vertical center of the screen
        let mid_y = (self.screen.h as i32) / 2;
        // Red in 0xRRGGBB
        let red = 0xFF0000;
        let green = 0x00FF00;
        let blue = 0x0000FF;

        for seg in &self.solid_segs {
            // Determine horizontal extents
            let mut x0 = seg.first;
            let mut x1 = seg.last;

            // Clamp to valid screen columns
            x0 = x0.max(0).min(self.screen.w as i32 - 1);
            x1 = x1.max(0).min(self.screen.w as i32 - 1);

            // Only draw if there's something to render
            if x0 <= x1 {
                self.renderer.draw_line(x0, mid_y, x1, mid_y, red);
                self.renderer.draw_line(x0, mid_y, x0, mid_y, green);
                self.renderer.draw_line(x1, mid_y, x1, mid_y, blue);
            }
        }
    }
}
