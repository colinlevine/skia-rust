//! Safe Rust implementation of `SkShadowTessellator` (`src/utils/SkShadowTessellator.cpp`)
//! and `SkShadowUtils` (`src/utils/SkShadowUtils.cpp`) geometry & color algorithms:
//! - Polygon sanitization, centroid/area accumulation, convexity detection
//! - Convex shadow mesh tessellation (umbra clipping, penumbra arcs, inner/outer rings)
//! - Concave shadow ring stitching
//! - Spot shadow curve clip-point evaluation
//! - Tonal shadow color computation, shadow rec filling & validation, ambient fallback params,
//!   spot occluder classification, and factory cache compatibility checks.

use crate::path_effects::float_round_to_int;
use crate::shadow_info::{
    ambient_blur_radius, ambient_recip_alpha, get_directional_params, get_spot_params,
    DrawShadowRec, SHADOW_FLAG_DIRECTIONAL_LIGHT,
};
use crate::{matrix_map_point, matrix_map_rect, point3_is_finite};
use skia_rs_geometry::{
    pin_f32, scalar_nearly_zero, Matrix, Point, Point3, Rect, Vector, SK_SCALAR_NEARLY_ZERO,
};

pub const K_PENUMBRA_COLOR: u32 = 0x0000_0000; // SK_ColorTRANSPARENT
pub const K_UMBRA_COLOR: u32 = 0xFF00_0000; // SK_ColorBLACK

#[inline]
fn sk_float_round(x: f32) -> f32 {
    ((x as f64) + 0.5).floor() as f32
}

/// Exact port of `sanitize_point` in `SkShadowTessellator.cpp`.
#[inline]
pub fn sanitize_point(pt: Point) -> Point {
    Point::new(
        sk_float_round(16.0 * pt.x) * 0.0625,
        sk_float_round(16.0 * pt.y) * 0.0625,
    )
}

/// Exact port of `duplicate_pt` in `SkShadowTessellator.cpp`.
#[inline]
pub fn duplicate_pt(p0: Point, p1: Point) -> bool {
    const K_CLOSE: f32 = 1.0 / 16.0;
    const K_CLOSE_SQD: f32 = K_CLOSE * K_CLOSE;
    Point::distance_to_sqd(p0, p1) < K_CLOSE_SQD
}

/// Exact port of `perp_dot` in `SkShadowTessellator.cpp`.
#[inline]
pub fn perp_dot(p0: Point, p1: Point, p2: Point) -> f32 {
    let v0 = Point::new(p1.x - p0.x, p1.y - p0.y);
    let v1 = Point::new(p2.x - p1.x, p2.y - p1.y);
    v0.cross(v1)
}

/// Exact port of `compute_normal` in `SkShadowTessellator.cpp`.
#[inline]
pub fn compute_normal(p0: Point, p1: Point, dir: f32) -> Option<Vector> {
    let mut normal = Vector::new((p0.y - p1.y) * dir, (p1.x - p0.x) * dir);
    if !normal.normalize() {
        return None;
    }
    Some(normal)
}

/// Exact port of `SkPMLerp` / `SkFastFourByteInterp256_64`.
#[inline]
pub fn pm_lerp(src: u32, dst: u32, scale: u32) -> u32 {
    #[inline]
    fn splay(c: u32) -> u64 {
        let mask: u64 = 0x00FF_00FF;
        let c64 = c as u64;
        (((c64 >> 8) & mask) << 32) | (c64 & mask)
    }
    #[inline]
    fn unsplay(agrb: u64) -> u32 {
        let mask: u64 = 0xFF00_FF00;
        (((agrb & mask) >> 8) | ((agrb >> 32) & mask)) as u32
    }
    let s = scale as u64;
    unsplay(splay(src).wrapping_mul(s).wrapping_add(splay(dst).wrapping_mul(256u64.wrapping_sub(s))))
}

/// Exact port of `SkComputeRadialSteps` used by `SkBaseShadowTessellator::addArc`.
pub fn compute_radial_steps(v1: Vector, v2: Vector, offset: f32) -> Option<(f32, f32, i32)> {
    const K_RECIP_PIXELS_PER_ARC_SEGMENT: f32 = 0.25;
    let r_cos = v1.dot(v2);
    if !r_cos.is_finite() {
        return None;
    }
    let r_sin = v1.cross(v2);
    if !r_sin.is_finite() {
        return None;
    }
    let theta = r_sin.atan2(r_cos);
    let float_steps = (offset * theta * K_RECIP_PIXELS_PER_ARC_SEGMENT).abs();
    if float_steps >= (u16::MAX as f32) {
        return None;
    }
    let steps = float_round_to_int(float_steps);
    let d_theta = if steps > 0 { theta / (steps as f32) } else { 0.0 };
    let rot_sin = d_theta.sin();
    let rot_cos = d_theta.cos();
    if steps > 0 && (rot_sin == 0.0 || rot_cos == 1.0) {
        return None;
    }
    Some((rot_sin, rot_cos, steps))
}

/// Computes `(inset, outset)` for `SkAmbientShadowTessellator`.
#[inline]
pub fn ambient_inset_outset(z_plane_params: Point3, bounds: Rect) -> (f32, f32) {
    let base_z = z_plane_params.x * bounds.center_x()
        + z_plane_params.y * bounds.center_y()
        + z_plane_params.z;
    let outset = ambient_blur_radius(base_z);
    let inset = outset * ambient_recip_alpha(base_z) - outset;
    (inset, outset)
}

/// Validates arguments to `SkShadowTessellator::MakeAmbient`.
#[inline]
pub fn validate_ambient_args(bounds: Rect, ctm: &Matrix, z_plane: Point3) -> bool {
    matrix_map_rect(ctm, bounds).is_finite() && point3_is_finite(z_plane)
}

/// Validates arguments to `SkShadowTessellator::MakeSpot`.
#[inline]
pub fn validate_spot_args(
    bounds: Rect,
    ctm: &Matrix,
    z_plane: Point3,
    light_pos: Point3,
    light_radius: f32,
) -> bool {
    matrix_map_rect(ctm, bounds).is_finite()
        && point3_is_finite(z_plane)
        && point3_is_finite(light_pos)
        && (light_pos.z >= SK_SCALAR_NEARLY_ZERO)
        && light_radius.is_finite()
        && (light_radius >= SK_SCALAR_NEARLY_ZERO)
}

/// Evaluates curve midpoint(s) for `SkSpotShadowTessellator::computeClipAndPathPolygons`.
#[inline]
pub fn spot_clip_quad_mid(p0: Point, p1: Point, p2: Point) -> Point {
    Point::new(
        0.25 * p0.x + 0.5 * p1.x + 0.25 * p2.x,
        0.25 * p0.y + 0.5 * p1.y + 0.25 * p2.y,
    )
}

#[inline]
pub fn spot_clip_conic_mid(p0: Point, p1: Point, p2: Point, w: f32) -> Point {
    let inv = 1.0_f32 / (0.5 + 0.5 * w);
    Point::new(
        (0.25 * p0.x + w * 0.5 * p1.x + 0.25 * p2.x) * inv,
        (0.25 * p0.y + w * 0.5 * p1.y + 0.25 * p2.y) * inv,
    )
}

#[inline]
pub fn spot_clip_cubic_mids(p0: Point, p1: Point, p2: Point, p3: Point) -> [Point; 2] {
    const K_A: f32 = 0.32495117187;
    const K_B: f32 = 0.44311523437;
    const K_C: f32 = 0.20141601562;
    const K_D: f32 = 0.03051757812;
    [
        Point::new(
            K_A * p0.x + K_B * p1.x + K_C * p2.x + K_D * p3.x,
            K_A * p0.y + K_B * p1.y + K_C * p2.y + K_D * p3.y,
        ),
        Point::new(
            K_D * p0.x + K_C * p1.x + K_B * p2.x + K_A * p3.x,
            K_D * p0.y + K_C * p1.y + K_B * p2.y + K_A * p3.y,
        ),
    ]
}

// ============================================================================
// Polygon & centroid state (exact match to SkBaseShadowTessellator)
// ============================================================================

/// Accumulates sanitized polygon vertices, centroid, signed area, and convexity.
#[derive(Debug, Clone)]
pub struct ShadowPolygonState {
    pub path_polygon: Vec<Point>,
    pub clip_polygon: Vec<Point>,
    pub centroid: Point,
    pub area: f32,
    pub last_area: f32,
    pub last_cross: f32,
    pub is_convex: bool,
    pub direction: f32,
}

impl Default for ShadowPolygonState {
    fn default() -> Self {
        Self::new()
    }
}

impl ShadowPolygonState {
    pub fn new() -> Self {
        Self {
            path_polygon: Vec::new(),
            clip_polygon: Vec::new(),
            centroid: Point::new(0.0, 0.0),
            area: 0.0,
            last_area: 0.0,
            last_cross: 0.0,
            is_convex: true,
            direction: 1.0,
        }
    }

    pub fn accumulate_centroid(&mut self, curr: Point, next: Point) -> bool {
        if duplicate_pt(curr, next) {
            return false;
        }
        let p0 = self.path_polygon[0];
        let v0 = Point::new(curr.x - p0.x, curr.y - p0.y);
        let v1 = Point::new(next.x - p0.x, next.y - p0.y);
        let quad_area = v0.cross(v1);
        self.centroid.x += (v0.x + v1.x) * quad_area;
        self.centroid.y += (v0.y + v1.y) * quad_area;
        self.area += quad_area;
        if quad_area * self.last_area < 0.0 {
            self.is_convex = false;
        }
        if quad_area != 0.0 {
            self.last_area = quad_area;
        }
        true
    }

    pub fn check_convexity(&mut self, p0: Point, p1: Point, p2: Point) -> bool {
        let cross = perp_dot(p0, p1, p2);
        if scalar_nearly_zero(cross, SK_SCALAR_NEARLY_ZERO) {
            return false;
        }
        if self.last_cross * cross < 0.0 {
            self.is_convex = false;
        }
        if cross != 0.0 {
            self.last_cross = cross;
        }
        true
    }

    pub fn handle_line(&mut self, p: Point) {
        let p_sanitized = sanitize_point(p);
        if let Some(&last) = self.path_polygon.last() {
            if !self.accumulate_centroid(last, p_sanitized) {
                return;
            }
        }
        if self.path_polygon.len() > 1 {
            let len = self.path_polygon.len();
            if !self.check_convexity(self.path_polygon[len - 2], self.path_polygon[len - 1], p_sanitized) {
                self.path_polygon.pop();
                if let Some(&prev) = self.path_polygon.last() {
                    if duplicate_pt(prev, p_sanitized) {
                        self.path_polygon.pop();
                    }
                }
            }
        }
        self.path_polygon.push(p_sanitized);
    }

    pub fn add_to_clip(&mut self, pt: Point) {
        if let Some(&last) = self.clip_polygon.last() {
            if duplicate_pt(pt, last) {
                return;
            }
        }
        self.clip_polygon.push(pt);
    }

    pub fn finish_path_polygon(&mut self) {
        if self.path_polygon.len() > 1 {
            let last = *self.path_polygon.last().unwrap();
            let first = self.path_polygon[0];
            if !self.accumulate_centroid(last, first) {
                self.path_polygon.pop();
            }
        }

        if self.path_polygon.len() > 2 {
            let scale = 1.0_f32 / (3.0 * self.area);
            self.centroid.x = self.centroid.x * scale + self.path_polygon[0].x;
            self.centroid.y = self.centroid.y * scale + self.path_polygon[0].y;
            let len = self.path_polygon.len();
            if !self.check_convexity(
                self.path_polygon[len - 2],
                self.path_polygon[len - 1],
                self.path_polygon[0],
            ) {
                self.path_polygon[0] = self.path_polygon[len - 1];
                self.path_polygon.pop();
            }
        }

        self.direction = if self.area > 0.0 { -1.0 } else { 1.0 };
    }
}

/// Computes minimum squared distance from `centroid` to any edge of `path_polygon`, and adjusts
/// `(inset, umbra_color)` if the umbra would collapse.
pub fn convex_prepare_inset_and_color(
    path_polygon: &[Point],
    centroid: Point,
    mut inset: f32,
) -> (f32, u32) {
    let mut umbra_color = K_UMBRA_COLOR;
    if path_polygon.len() < 2 {
        return (inset, umbra_color);
    }
    let mut min_dist_sq =
        Point::distance_to_line_segment_between_sqd(centroid, path_polygon[0], path_polygon[1]);
    for i in 1..path_polygon.len() {
        let j = if i == path_polygon.len() - 1 { 0 } else { i + 1 };
        let d = Point::distance_to_line_segment_between_sqd(
            centroid,
            path_polygon[i],
            path_polygon[j],
        );
        if d < min_dist_sq {
            min_dist_sq = d;
        }
    }

    if inset > SK_SCALAR_NEARLY_ZERO {
        const K_TOLERANCE: f32 = 1.0e-2;
        if min_dist_sq < (inset + K_TOLERANCE) * (inset + K_TOLERANCE) {
            let new_inset = min_dist_sq.sqrt() - K_TOLERANCE;
            let ratio = 128.0 * (new_inset / inset + 1.0);
            umbra_color = pm_lerp(K_UMBRA_COLOR, K_PENUMBRA_COLOR, ratio as u32);
            inset = new_inset;
        }
    }
    (inset, umbra_color)
}

/// Output mesh of shadow tessellation.
#[derive(Debug, Clone, Default)]
pub struct ShadowMesh {
    pub positions: Vec<Point>,
    pub colors: Vec<u32>,
    pub indices: Vec<u16>,
    pub index_map: Vec<u16>,
}

/// Exact port of `SkBaseShadowTessellator::computeConvexShadow` (after `SkInsetConvexPolygon`
/// has optionally been called by the caller to produce `umbra_polygon` / `valid_umbra`).
#[allow(clippy::too_many_arguments)]
pub fn build_convex_shadow_mesh(
    path_polygon: &[Point],
    umbra_polygon: &[Point],
    valid_umbra: bool,
    clip_polygon: &[Point],
    centroid: Point,
    direction: f32,
    outset: f32,
    umbra_color: u32,
    mut transparent: bool,
    do_clip: bool,
) -> Option<ShadowMesh> {
    let mut clip_vectors = Vec::new();
    let mut curr_clip_index: usize = 0;

    if do_clip {
        debug_assert!(clip_polygon.len() >= 3);
        curr_clip_index = clip_polygon.len() - 1;
        let mut v0 = Point::new(
            clip_polygon[1].x - clip_polygon[0].x,
            clip_polygon[1].y - clip_polygon[0].y,
        );
        clip_vectors.push(v0);

        let mut hidden_centroid = true;
        let mut v1 = Point::new(centroid.x - clip_polygon[0].x, centroid.y - clip_polygon[0].y);
        let init_cross = v0.cross(v1);

        for p in 1..clip_polygon.len() {
            let next_p = (p + 1) % clip_polygon.len();
            v0 = Point::new(
                clip_polygon[next_p].x - clip_polygon[p].x,
                clip_polygon[next_p].y - clip_polygon[p].y,
            );
            clip_vectors.push(v0);
            v1 = Point::new(centroid.x - clip_polygon[p].x, centroid.y - clip_polygon[p].y);
            if init_cross * v0.cross(v1) <= 0.0 {
                hidden_centroid = false;
            }
        }
        transparent = transparent || !hidden_centroid;
    }

    let mut mesh = ShadowMesh::default();
    if transparent {
        mesh.positions.push(centroid);
        mesh.colors.push(umbra_color);
    }

    let poly_count = path_polygon.len();
    let mut first_outset = compute_normal(path_polygon[poly_count - 1], path_polygon[0], direction)?;
    first_outset.x *= outset;
    first_outset.y *= outset;

    let first_point = path_polygon[poly_count - 1];
    let first_vertex_index = mesh.positions.len() as i32;
    let mut prev_outset = first_outset;
    let mut prev_point = first_point;
    let mut prev_umbra_index: i32 = -1;
    let mut curr_umbra_search: usize = 0;
    let mut prev_umbra_outside = false;
    let mut first_umbra_outside = false;

    let append_triangle = |indices: &mut Vec<u16>, i0: i32, i1: i32, i2: i32| {
        indices.push(i0 as u16);
        indices.push(i1 as u16);
        indices.push(i2 as u16);
    };

    let clip_umbra = |umbra_point: Point, curr_clip: &mut usize| -> Option<Point> {
        let segment_vector = Point::new(centroid.x - umbra_point.x, centroid.y - umbra_point.y);
        let start = *curr_clip;
        loop {
            let dp = Point::new(
                umbra_point.x - clip_polygon[*curr_clip].x,
                umbra_point.y - clip_polygon[*curr_clip].y,
            );
            let denom = clip_vectors[*curr_clip].cross(segment_vector);
            let t_num = dp.cross(segment_vector);
            if scalar_nearly_zero(denom, SK_SCALAR_NEARLY_ZERO) {
                if scalar_nearly_zero(t_num, SK_SCALAR_NEARLY_ZERO) {
                    return None;
                }
            } else if t_num >= 0.0 && t_num <= denom {
                let s_num = dp.cross(clip_vectors[*curr_clip]);
                if s_num >= 0.0 && s_num <= denom {
                    let s = s_num / denom;
                    return Some(Point::new(
                        umbra_point.x + segment_vector.x * s,
                        umbra_point.y + segment_vector.y * s,
                    ));
                }
            }
            *curr_clip = (*curr_clip + 1) % clip_polygon.len();
            if *curr_clip == start {
                break;
            }
        }
        None
    };

    let get_closest_umbra = |p: Point, curr_idx: &mut usize| -> usize {
        let len = umbra_polygon.len();
        let mut min_dist = Point::distance_to_sqd(p, umbra_polygon[*curr_idx]);
        let mut index = *curr_idx;
        let mut dir = 1usize;
        let mut next = (index + dir) % len;
        let mut dist = Point::distance_to_sqd(p, umbra_polygon[next]);
        if dist < min_dist {
            index = next;
            min_dist = dist;
        } else {
            dir = len - 1;
        }
        next = (index + dir) % len;
        dist = Point::distance_to_sqd(p, umbra_polygon[next]);
        while dist < min_dist {
            index = next;
            min_dist = dist;
            next = (index + dir) % len;
            dist = Point::distance_to_sqd(p, umbra_polygon[next]);
        }
        *curr_idx = index;
        index
    };

    let add_inner_point = |path_point: Point,
                               positions: &mut Vec<Point>,
                               colors: &mut Vec<u32>,
                               prev_pt: &mut Point,
                               prev_u_idx: i32,
                               curr_search: &mut usize|
     -> (i32, bool) {
        let umbra_point = if !valid_umbra {
            let vx = (centroid.x - path_point.x) * 0.95;
            let vy = (centroid.y - path_point.y) * 0.95;
            Point::new(path_point.x + vx, path_point.y + vy)
        } else {
            umbra_polygon[get_closest_umbra(path_point, curr_search)]
        };
        *prev_pt = path_point;

        if prev_u_idx == -1 || !duplicate_pt(umbra_point, positions[prev_u_idx as usize]) {
            if prev_u_idx >= 0 && duplicate_pt(umbra_point, positions[first_vertex_index as usize]) {
                (first_vertex_index, false)
            } else {
                let idx = positions.len() as i32;
                positions.push(umbra_point);
                colors.push(umbra_color);
                (idx, false)
            }
        } else {
            (prev_u_idx, true)
        }
    };

    let (first_u_idx, _) = add_inner_point(
        first_point,
        &mut mesh.positions,
        &mut mesh.colors,
        &mut prev_point,
        prev_umbra_index,
        &mut curr_umbra_search,
    );
    prev_umbra_index = first_u_idx;

    if !transparent && do_clip {
        if let Some(clip_pt) = clip_umbra(mesh.positions[first_vertex_index as usize], &mut curr_clip_index) {
            mesh.positions.push(clip_pt);
            mesh.colors.push(umbra_color);
            prev_umbra_outside = true;
            first_umbra_outside = true;
        }
    }

    mesh.positions
        .push(Point::new(first_point.x + first_outset.x, first_point.y + first_outset.y));
    mesh.colors.push(K_PENUMBRA_COLOR);

    let add_arc = |next_normal: Vector,
                       finish_arc: bool,
                       positions: &mut Vec<Point>,
                       colors: &mut Vec<u32>,
                       indices: &mut Vec<u16>,
                       prev_out: &mut Vector,
                       prev_pt: Point,
                       prev_u: i32|
     -> bool {
        let (rot_sin, rot_cos, num_steps) =
            compute_radial_steps(*prev_out, next_normal, outset).unwrap_or((0.0, 1.0, 0));
        let mut prev_norm = *prev_out;
        for _ in 0..(num_steps - 1).max(0) {
            let curr_norm = Vector::new(
                prev_norm.x * rot_cos - prev_norm.y * rot_sin,
                prev_norm.y * rot_cos + prev_norm.x * rot_sin,
            );
            positions.push(Point::new(prev_pt.x + curr_norm.x, prev_pt.y + curr_norm.y));
            colors.push(K_PENUMBRA_COLOR);
            let n = positions.len() as i32;
            append_triangle(indices, prev_u, n - 1, n - 2);
            prev_norm = curr_norm;
        }
        if finish_arc && num_steps != 0 {
            positions.push(Point::new(prev_pt.x + next_normal.x, prev_pt.y + next_normal.y));
            colors.push(K_PENUMBRA_COLOR);
            let n = positions.len() as i32;
            append_triangle(indices, prev_u, n - 1, n - 2);
        }
        *prev_out = next_normal;
        num_steps > 0
    };

    let add_edge = |next_point: Point,
                        next_normal: Vector,
                        last_edge: bool,
                        mesh: &mut ShadowMesh,
                        prev_pt: &mut Point,
                        prev_u: &mut i32,
                        prev_out: &mut Vector,
                        prev_u_out: &mut bool,
                        curr_search: &mut usize,
                        curr_clip: &mut usize| {
        let (curr_u_idx, dup) = if last_edge {
            *prev_pt = next_point;
            (first_vertex_index, false)
        } else {
            add_inner_point(
                next_point,
                &mut mesh.positions,
                &mut mesh.colors,
                prev_pt,
                *prev_u,
                curr_search,
            )
        };
        let prev_penumbra_idx = if dup || curr_u_idx == first_vertex_index {
            mesh.positions.len() as i32 - 1
        } else {
            mesh.positions.len() as i32 - 2
        };

        if !dup {
            if transparent {
                append_triangle(&mut mesh.indices, 0, *prev_u, curr_u_idx);
            } else if do_clip {
                let (is_outside, maybe_clip_pt) = if last_edge {
                    (first_umbra_outside, None)
                } else {
                    match clip_umbra(mesh.positions[curr_u_idx as usize], curr_clip) {
                        Some(pt) => (true, Some(pt)),
                        None => (false, None),
                    }
                };
                if is_outside {
                    if let Some(pt) = maybe_clip_pt {
                        mesh.positions.push(pt);
                        mesh.colors.push(umbra_color);
                    }
                    append_triangle(&mut mesh.indices, *prev_u, curr_u_idx, curr_u_idx + 1);
                    if *prev_u_out {
                        append_triangle(&mut mesh.indices, *prev_u, curr_u_idx + 1, *prev_u + 1);
                    }
                } else if *prev_u_out {
                    append_triangle(&mut mesh.indices, *prev_u, curr_u_idx, *prev_u + 1);
                }
                *prev_u_out = is_outside;
            }
        }

        mesh.positions
            .push(Point::new(next_point.x + next_normal.x, next_point.y + next_normal.y));
        mesh.colors.push(K_PENUMBRA_COLOR);

        if !dup {
            append_triangle(&mut mesh.indices, *prev_u, prev_penumbra_idx, curr_u_idx);
        }
        let last_pos = mesh.positions.len() as i32 - 1;
        append_triangle(&mut mesh.indices, prev_penumbra_idx, last_pos, curr_u_idx);

        *prev_u = curr_u_idx;
        *prev_out = next_normal;
    };

    add_edge(
        path_polygon[0],
        first_outset,
        false,
        &mut mesh,
        &mut prev_point,
        &mut prev_umbra_index,
        &mut prev_outset,
        &mut prev_umbra_outside,
        &mut curr_umbra_search,
        &mut curr_clip_index,
    );

    for i in 1..poly_count {
        let mut normal = compute_normal(prev_point, path_polygon[i], direction)?;
        normal.x *= outset;
        normal.y *= outset;
        add_arc(
            normal,
            true,
            &mut mesh.positions,
            &mut mesh.colors,
            &mut mesh.indices,
            &mut prev_outset,
            prev_point,
            prev_umbra_index,
        );
        add_edge(
            path_polygon[i],
            normal,
            i == poly_count - 1,
            &mut mesh,
            &mut prev_point,
            &mut prev_umbra_index,
            &mut prev_outset,
            &mut prev_umbra_outside,
            &mut curr_umbra_search,
            &mut curr_clip_index,
        );
    }

    // Final fan
    if add_arc(
        first_outset,
        false,
        &mut mesh.positions,
        &mut mesh.colors,
        &mut mesh.indices,
        &mut prev_outset,
        prev_point,
        prev_umbra_index,
    ) {
        let last = mesh.positions.len() as i32 - 1;
        let target = if first_umbra_outside {
            first_vertex_index + 2
        } else {
            first_vertex_index + 1
        };
        append_triangle(&mut mesh.indices, first_vertex_index, last, target);
    } else {
        let last_pt = *mesh.positions.last().unwrap();
        let target = if first_umbra_outside {
            (first_vertex_index + 2) as usize
        } else {
            (first_vertex_index + 1) as usize
        };
        mesh.positions[target] = last_pt;
    }

    Some(mesh)
}

/// Exact port of concave inset clamping in `SkBaseShadowTessellator::computeConcaveShadow`.
#[inline]
pub fn concave_clamp_inset(inset: f32, path_bounds: Rect) -> f32 {
    let half_w = (path_bounds.right * 0.5 - path_bounds.left * 0.5).abs();
    let half_h = (path_bounds.bottom * 0.5 - path_bounds.top * 0.5).abs();
    let min_half = if half_h < half_w { half_h } else { half_w };
    if min_half < inset { min_half } else { inset }
}

/// Exact port of `SkBaseShadowTessellator::stitchConcaveRings` (leaving optional interior
/// triangulation of `umbra_polygon` via `index_map` to the caller if `transparent` is true).
pub fn stitch_concave_rings(
    path_poly_len: usize,
    umbra_polygon: &[Point],
    umbra_indices: &mut [i32],
    penumbra_polygon: &[Point],
    penumbra_indices: &mut [i32],
) -> ShadowMesh {
    let mut mesh = ShadowMesh {
        positions: Vec::new(),
        colors: Vec::new(),
        indices: Vec::new(),
        index_map: vec![0u16; umbra_polygon.len()],
    };

    let append_tri = |indices: &mut Vec<u16>, i0: usize, i1: usize, i2: usize| {
        indices.push(i0 as u16);
        indices.push(i1 as u16);
        indices.push(i2 as u16);
    };
    let append_quad = |indices: &mut Vec<u16>, i0: usize, i1: usize, i2: usize, i3: usize| {
        indices.push(i0 as u16);
        indices.push(i1 as u16);
        indices.push(i2 as u16);
        indices.push(i2 as u16);
        indices.push(i1 as u16);
        indices.push(i3 as u16);
    };

    let mut curr_penumbra = 0usize;
    let mut min_val = penumbra_indices[0];
    for (i, &v) in penumbra_indices.iter().enumerate().skip(1) {
        if v < min_val {
            min_val = v;
            curr_penumbra = i;
        }
    }

    let mut curr_umbra = 0usize;
    let mut min_u = umbra_indices[0];
    for (i, &v) in umbra_indices.iter().enumerate().skip(1) {
        if v < min_u {
            min_u = v;
            curr_umbra = i;
        }
    }

    let poly_n = path_poly_len as i32;
    let mut max_penumbra_index = poly_n - 1;
    let mut max_umbra_index = poly_n - 1;
    while penumbra_indices[curr_penumbra] != umbra_indices[curr_umbra] {
        if penumbra_indices[curr_penumbra] < umbra_indices[curr_umbra] {
            penumbra_indices[curr_penumbra] += poly_n;
            max_penumbra_index = penumbra_indices[curr_penumbra];
            curr_penumbra = (curr_penumbra + 1) % penumbra_polygon.len();
        } else {
            umbra_indices[curr_umbra] += poly_n;
            max_umbra_index = umbra_indices[curr_umbra];
            curr_umbra = (curr_umbra + 1) % umbra_polygon.len();
        }
    }

    mesh.positions.push(penumbra_polygon[curr_penumbra]);
    mesh.colors.push(K_PENUMBRA_COLOR);
    let mut prev_penumbra_index = 0usize;
    mesh.positions.push(umbra_polygon[curr_umbra]);
    mesh.colors.push(K_UMBRA_COLOR);
    let mut prev_umbra_index = 1usize;
    mesh.index_map[curr_umbra] = 1;

    let mut next_penumbra = (curr_penumbra + 1) % penumbra_polygon.len();
    let mut next_umbra = (curr_umbra + 1) % umbra_polygon.len();

    while penumbra_indices[next_penumbra] <= max_penumbra_index
        || umbra_indices[next_umbra] <= max_umbra_index
    {
        if umbra_indices[next_umbra] == penumbra_indices[next_penumbra] {
            mesh.positions.push(penumbra_polygon[next_penumbra]);
            mesh.colors.push(K_PENUMBRA_COLOR);
            let curr_p_idx = mesh.positions.len() - 1;

            mesh.positions.push(umbra_polygon[next_umbra]);
            mesh.colors.push(K_UMBRA_COLOR);
            let curr_u_idx = mesh.positions.len() - 1;
            mesh.index_map[next_umbra] = curr_u_idx as u16;

            append_quad(&mut mesh.indices, prev_penumbra_index, curr_p_idx, prev_umbra_index, curr_u_idx);

            prev_penumbra_index = curr_p_idx;
            penumbra_indices[curr_penumbra] += poly_n;
            curr_penumbra = next_penumbra;
            next_penumbra = (curr_penumbra + 1) % penumbra_polygon.len();

            prev_umbra_index = curr_u_idx;
            umbra_indices[curr_umbra] += poly_n;
            curr_umbra = next_umbra;
            next_umbra = (curr_umbra + 1) % umbra_polygon.len();
        }

        while penumbra_indices[next_penumbra] < umbra_indices[next_umbra]
            && penumbra_indices[next_penumbra] <= max_penumbra_index
        {
            mesh.positions.push(penumbra_polygon[next_penumbra]);
            mesh.colors.push(K_PENUMBRA_COLOR);
            let curr_p_idx = mesh.positions.len() - 1;

            append_tri(&mut mesh.indices, prev_penumbra_index, curr_p_idx, prev_umbra_index);

            prev_penumbra_index = curr_p_idx;
            penumbra_indices[curr_penumbra] += poly_n;
            curr_penumbra = next_penumbra;
            next_penumbra = (curr_penumbra + 1) % penumbra_polygon.len();
        }

        while umbra_indices[next_umbra] < penumbra_indices[next_penumbra]
            && umbra_indices[next_umbra] <= max_umbra_index
        {
            mesh.positions.push(umbra_polygon[next_umbra]);
            mesh.colors.push(K_UMBRA_COLOR);
            let curr_u_idx = mesh.positions.len() - 1;
            mesh.index_map[next_umbra] = curr_u_idx as u16;

            append_tri(&mut mesh.indices, prev_umbra_index, prev_penumbra_index, curr_u_idx);

            prev_umbra_index = curr_u_idx;
            umbra_indices[curr_umbra] += poly_n;
            curr_umbra = next_umbra;
            next_umbra = (curr_umbra + 1) % umbra_polygon.len();
        }
    }

    mesh.positions.push(penumbra_polygon[next_penumbra]);
    mesh.colors.push(K_PENUMBRA_COLOR);
    let curr_p_idx = mesh.positions.len() - 1;

    mesh.positions.push(umbra_polygon[next_umbra]);
    mesh.colors.push(K_UMBRA_COLOR);
    let curr_u_idx = mesh.positions.len() - 1;
    mesh.index_map[next_umbra] = curr_u_idx as u16;

    append_quad(&mut mesh.indices, prev_penumbra_index, curr_p_idx, prev_umbra_index, curr_u_idx);

    mesh
}

// ============================================================================
// SkShadowUtils computational helpers
// ============================================================================

/// Exact port of `SkShadowUtils::ComputeTonalColors`.
pub fn compute_tonal_colors(in_ambient: u32, in_spot: u32) -> (u32, u32) {
    let out_ambient = ((in_ambient >> 24) & 0xFF) << 24;

    let spot_r = ((in_spot >> 16) & 0xFF) as i32;
    let spot_g = ((in_spot >> 8) & 0xFF) as i32;
    let spot_b = (in_spot & 0xFF) as i32;
    let max_c = spot_r.max(spot_g).max(spot_b);
    let min_c = spot_r.min(spot_g).min(spot_b);
    let luminance = 0.5_f32 * ((max_c + min_c) as f32) / 255.0_f32;
    let orig_a = (((in_spot >> 24) & 0xFF) as f32) / 255.0_f32;

    let alpha_adjust = (2.6_f32 + (-2.66667_f32 + 1.06667_f32 * orig_a) * orig_a) * orig_a;
    let mut color_alpha =
        (3.544762_f32 + (-4.891428_f32 + 2.3466_f32 * luminance) * luminance) * luminance;
    color_alpha = pin_f32(alpha_adjust * color_alpha, 0.0, 1.0);

    let greyscale_alpha = pin_f32(orig_a * (1.0 - 0.4 * luminance), 0.0, 1.0);

    let color_scale = color_alpha * (1.0 - greyscale_alpha);
    let tonal_alpha = color_scale + greyscale_alpha;
    let un_premul_scale = color_scale / tonal_alpha;

    let a = ((tonal_alpha * 255.999_f32) as u32) & 0xFF;
    let r = ((un_premul_scale * (spot_r as f32)) as u32) & 0xFF;
    let g = ((un_premul_scale * (spot_g as f32)) as u32) & 0xFF;
    let b = ((un_premul_scale * (spot_b as f32)) as u32) & 0xFF;
    let out_spot = (a << 24) | (r << 16) | (g << 8) | b;

    (out_ambient, out_spot)
}

/// Exact port of `fill_shadow_rec` in `SkShadowUtils.cpp`.
#[allow(clippy::too_many_arguments)]
pub fn fill_shadow_rec(
    z_plane_params: Point3,
    light_pos: Point3,
    light_radius: f32,
    ambient_color: u32,
    spot_color: u32,
    flags: u32,
    ctm: &Matrix,
) -> Option<DrawShadowRec> {
    let mut pt = Point::new(light_pos.x, light_pos.y);
    if (flags & SHADOW_FLAG_DIRECTIONAL_LIGHT) == 0 {
        let inv = ctm.invert()?;
        pt = matrix_map_point(&inv, pt);
    }
    Some(DrawShadowRec {
        z_plane_params,
        light_pos: Point3::new(pt.x, pt.y, light_pos.z),
        light_radius,
        ambient_color,
        spot_color,
        flags,
    })
}

/// Exact port of `validate_rec` in `SkShadowUtils.cpp`.
#[inline]
pub fn validate_shadow_rec(rec: &DrawShadowRec) -> bool {
    point3_is_finite(rec.light_pos)
        && point3_is_finite(rec.z_plane_params)
        && rec.light_radius.is_finite()
}

/// Exact port of `tilted` in `SkShadowUtils.cpp`.
#[inline]
pub fn is_z_plane_tilted(z_plane_params: Point3) -> bool {
    !scalar_nearly_zero(z_plane_params.x, SK_SCALAR_NEARLY_ZERO)
        || !scalar_nearly_zero(z_plane_params.y, SK_SCALAR_NEARLY_ZERO)
}

/// Computes `(blur_radius, stroke_width)` for the ambient blur fallback in `SkDevice::drawShadow`.
#[inline]
pub fn ambient_blur_fallback_params(z_plane_z: f32) -> (f32, f32) {
    let dev_space_outset = ambient_blur_radius(z_plane_z);
    let one_over_a = ambient_recip_alpha(z_plane_z);
    let blur_radius = 0.5 * dev_space_outset * one_over_a;
    let stroke_width = 0.5 * (dev_space_outset - blur_radius);
    (blur_radius, stroke_width)
}

/// Spot occluder classification mirroring `SpotVerticesFactory::OccluderType`.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpotOccluderType {
    PointTransparent = 0,
    PointOpaquePartialUmbra = 1,
    PointOpaqueNoUmbra = 2,
    Directional = 3,
    DirectionalTransparent = 4,
}

/// Result of classifying a spot shadow for caching in `SkDevice::drawShadow`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpotFactoryParams {
    pub local_center: Point,
    pub offset: Vector,
    pub occluder_type: i32,
}

/// Exact port of spot factory classification in `SkDevice::drawShadow`.
#[allow(clippy::too_many_arguments)]
pub fn compute_spot_factory_params(
    path_bounds: Rect,
    path_is_convex: bool,
    view_matrix: &Matrix,
    z_plane_z: f32,
    dev_light_pos: Point3,
    light_radius: f32,
    transparent: bool,
    directional: bool,
) -> SpotFactoryParams {
    let local_center = Point::new(path_bounds.center_x(), path_bounds.center_y());
    let center = matrix_map_point(view_matrix, local_center);
    let (radius, scale, mut offset) = if directional {
        get_directional_params(
            z_plane_z,
            dev_light_pos.x,
            dev_light_pos.y,
            dev_light_pos.z,
            light_radius,
        )
    } else {
        get_spot_params(
            z_plane_z,
            dev_light_pos.x - center.x,
            dev_light_pos.y - center.y,
            dev_light_pos.z,
            light_radius,
        )
    };

    let dev_bounds = matrix_map_rect(view_matrix, path_bounds);
    let occluder_type = if transparent
        || offset.x.abs() > 0.5 * dev_bounds.width()
        || offset.y.abs() > 0.5 * dev_bounds.height()
    {
        if directional {
            SpotOccluderType::DirectionalTransparent
        } else {
            SpotOccluderType::PointTransparent
        }
    } else if directional {
        SpotOccluderType::Directional
    } else if offset.mag() * scale + scale < radius {
        SpotOccluderType::PointOpaqueNoUmbra
    } else if path_is_convex {
        SpotOccluderType::PointOpaquePartialUmbra
    } else {
        SpotOccluderType::PointTransparent
    };

    offset.x += view_matrix.mat[2];
    offset.y += view_matrix.mat[5];

    SpotFactoryParams {
        local_center,
        offset,
        occluder_type: occluder_type as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_tonal_colors() {
        let (amb, spot) = compute_tonal_colors(0x40FF0000, 0x40FF0000);
        // Ambient is grayscale black with original alpha
        assert_eq!(amb, 0x40000000);
        // Spot has non-zero alpha and red channel, zero green/blue
        assert!((spot >> 24) > 0);
        assert!(((spot >> 16) & 0xFF) > 0);
        assert_eq!((spot >> 8) & 0xFF, 0);
        assert_eq!(spot & 0xFF, 0);

        // Zero alpha gives zero alpha
        let (amb0, spot0) = compute_tonal_colors(0x00112233, 0x00000000);
        assert_eq!(amb0, 0);
        assert_eq!(spot0 >> 24, 0);
    }

    #[test]
    fn test_polygon_state_square_and_convex_mesh() {
        let mut poly = ShadowPolygonState::new();
        poly.handle_line(Point::new(0.0, 0.0));
        poly.handle_line(Point::new(100.0, 0.0));
        poly.handle_line(Point::new(100.0, 100.0));
        poly.handle_line(Point::new(0.0, 100.0));
        poly.finish_path_polygon();

        assert!(poly.is_convex);
        assert_eq!(poly.path_polygon.len(), 4);
        assert!((poly.centroid.x - 50.0).abs() < 1e-3);
        assert!((poly.centroid.y - 50.0).abs() < 1e-3);

        let mesh = build_convex_shadow_mesh(
            &poly.path_polygon,
            &poly.path_polygon,
            true,
            &[],
            poly.centroid,
            poly.direction,
            10.0,
            K_UMBRA_COLOR,
            true,
            false,
        )
        .unwrap();
        assert!(!mesh.positions.is_empty());
        assert_eq!(mesh.positions.len(), mesh.colors.len());
        assert!(!mesh.indices.is_empty());
        assert_eq!(mesh.indices.len() % 3, 0);
    }

    #[test]
    fn test_stitch_concave_rings_basic() {
        let umbra = [
            Point::new(10.0, 10.0),
            Point::new(90.0, 10.0),
            Point::new(90.0, 90.0),
            Point::new(10.0, 90.0),
        ];
        let penumbra = [
            Point::new(-10.0, -10.0),
            Point::new(110.0, -10.0),
            Point::new(110.0, 110.0),
            Point::new(-10.0, 110.0),
        ];
        let mut u_idx = [0, 1, 2, 3];
        let mut p_idx = [0, 1, 2, 3];
        let mesh = stitch_concave_rings(4, &umbra, &mut u_idx, &penumbra, &mut p_idx);
        assert_eq!(mesh.positions.len(), 10);
        assert_eq!(mesh.indices.len(), 24);
    }

    #[test]
    fn test_pm_lerp() {
        assert_eq!(pm_lerp(K_UMBRA_COLOR, K_PENUMBRA_COLOR, 256), K_UMBRA_COLOR);
        assert_eq!(pm_lerp(K_UMBRA_COLOR, K_PENUMBRA_COLOR, 0), K_PENUMBRA_COLOR);
        assert_eq!(pm_lerp(K_UMBRA_COLOR, K_PENUMBRA_COLOR, 128), 0x7F00_0000);
    }

    #[test]
    fn test_shadow_utils_helpers() {
        let m = Matrix::identity();
        let rec = fill_shadow_rec(
            Point3::new(0.0, 0.0, 12.0),
            Point3::new(50.0, 60.0, 600.0),
            80.0,
            0x20000000,
            0x40000000,
            0,
            &m,
        )
        .unwrap();
        assert!(validate_shadow_rec(&rec));
        assert!(!is_z_plane_tilted(rec.z_plane_params));
        assert!(is_z_plane_tilted(Point3::new(0.1, 0.0, 12.0)));

        let (blur_r, stroke_w) = ambient_blur_fallback_params(12.0);
        assert!(blur_r > 0.0);
        assert!(stroke_w < blur_r);

        let params = compute_spot_factory_params(
            Rect::make_ltrb(0.0, 0.0, 100.0, 100.0),
            true,
            &m,
            12.0,
            Point3::new(50.0, 50.0, 600.0),
            80.0,
            false,
            false,
        );
        assert_eq!(params.local_center, Point::new(50.0, 50.0));
        assert_eq!(params.occluder_type, SpotOccluderType::PointOpaqueNoUmbra as i32);

        let params_dir = compute_spot_factory_params(
            Rect::make_ltrb(0.0, 0.0, 100.0, 100.0),
            true,
            &m,
            12.0,
            Point3::new(0.0, -1.0, 1.0),
            0.5,
            false,
            true,
        );
        assert_eq!(params_dir.occluder_type, SpotOccluderType::Directional as i32);
    }

    #[test]
    fn test_convex_shadow_mesh_with_clip() {
        let mut poly = ShadowPolygonState::new();
        for p in [
            Point::new(0.0, 0.0),
            Point::new(100.0, 0.0),
            Point::new(100.0, 100.0),
            Point::new(0.0, 100.0),
        ] {
            poly.handle_line(p);
            poly.add_to_clip(Point::new(p.x - 5.0, p.y - 5.0));
        }
        poly.finish_path_polygon();

        let (inset, umbra_color) = convex_prepare_inset_and_color(&poly.path_polygon, poly.centroid, 8.0);
        assert_eq!(inset, 8.0);
        assert_eq!(umbra_color, K_UMBRA_COLOR);

        let umbra = [
            Point::new(8.0, 8.0),
            Point::new(92.0, 8.0),
            Point::new(92.0, 92.0),
            Point::new(8.0, 92.0),
        ];
        let mesh = build_convex_shadow_mesh(
            &poly.path_polygon,
            &umbra,
            true,
            &poly.clip_polygon,
            poly.centroid,
            poly.direction,
            8.0,
            umbra_color,
            false,
            true,
        )
        .unwrap();
        assert!(!mesh.positions.is_empty());
        assert_eq!(mesh.positions.len(), mesh.colors.len());
        assert_eq!(mesh.indices.len() % 3, 0);
    }
}
