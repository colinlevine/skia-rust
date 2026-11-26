//! Safe Rust implementation of `SkDrawShadowInfo` (`src/core/SkDrawShadowInfo.cpp` & `.h`):
//! - Ambient & spot shadow radius/alpha/scale/translation metrics
//! - `GetSpotShadowTransform` (affine, directional, and 3D perspective quad projection)
//! - `GetLocalBounds`

use crate::{matrix_map_point, matrix_map_rect, matrix_map_vector};
use skia_rs_geometry::{
    cpp_max_f32, cpp_min_f32, pin_f32, scalar_nearly_zero, Matrix, Point, Point3, Rect, Vector,
    SK_SCALAR_NEARLY_ZERO,
};

pub const K_AMBIENT_HEIGHT_FACTOR: f32 = 1.0 / 128.0;
pub const K_AMBIENT_GEOM_FACTOR: f32 = 64.0;
pub const K_MAX_AMBIENT_RADIUS: f32 = 300.0 * K_AMBIENT_HEIGHT_FACTOR * K_AMBIENT_GEOM_FACTOR;

pub const SHADOW_FLAG_TRANSPARENT_OCCLUDER: u32 = 0x01;
pub const SHADOW_FLAG_GEOMETRIC_ONLY: u32 = 0x02;
pub const SHADOW_FLAG_DIRECTIONAL_LIGHT: u32 = 0x04;
pub const SHADOW_FLAG_CONCAVE_BLUR_ONLY: u32 = 0x08;

/// Mirrors `SkDrawShadowRec` (`#[repr(C)]`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawShadowRec {
    pub z_plane_params: Point3,
    pub light_pos: Point3,
    pub light_radius: f32,
    pub ambient_color: u32,
    pub spot_color: u32,
    pub flags: u32,
}

#[inline]
fn divide_and_pin(numer: f32, denom: f32, min: f32, max: f32) -> f32 {
    pin_f32(numer / denom, min, max)
}

#[inline]
pub fn ambient_blur_radius(height: f32) -> f32 {
    cpp_min_f32(
        height * K_AMBIENT_HEIGHT_FACTOR * K_AMBIENT_GEOM_FACTOR,
        K_MAX_AMBIENT_RADIUS,
    )
}

#[inline]
pub fn ambient_recip_alpha(height: f32) -> f32 {
    1.0 + cpp_max_f32(height * K_AMBIENT_HEIGHT_FACTOR, 0.0)
}

#[inline]
pub fn spot_blur_radius(occluder_z: f32, light_z: f32, light_radius: f32) -> f32 {
    light_radius * divide_and_pin(occluder_z, light_z - occluder_z, 0.0, 0.95)
}

#[inline]
pub fn get_spot_params(
    occluder_z: f32,
    light_x: f32,
    light_y: f32,
    light_z: f32,
    light_radius: f32,
) -> (f32, f32, Vector) {
    let z_ratio = divide_and_pin(occluder_z, light_z - occluder_z, 0.0, 0.95);
    let blur_radius = light_radius * z_ratio;
    let scale = divide_and_pin(light_z, light_z - occluder_z, 1.0, 1.95);
    let translate = Vector::new(-z_ratio * light_x, -z_ratio * light_y);
    (blur_radius, scale, translate)
}

#[inline]
pub fn get_directional_params(
    occluder_z: f32,
    light_x: f32,
    light_y: f32,
    light_z: f32,
    light_radius: f32,
) -> (f32, f32, Vector) {
    let blur_radius = light_radius * occluder_z;
    let scale = 1.0_f32;
    let max_z_ratio = 64.0_f32 / SK_SCALAR_NEARLY_ZERO;
    let z_ratio = divide_and_pin(occluder_z, light_z, 0.0, max_z_ratio);
    let translate = Vector::new(-z_ratio * light_x, -z_ratio * light_y);
    (blur_radius, scale, translate)
}

#[inline]
fn compute_z(x: f32, y: f32, params: Point3) -> f32 {
    x * params.x + y * params.y + params.z
}

/// Exact port of `SkDrawShadowMetrics::GetSpotShadowTransform`.
pub fn get_spot_shadow_transform(
    light_pos: Point3,
    light_radius: f32,
    ctm: &Matrix,
    z_plane_params: Point3,
    path_bounds: Rect,
    directional: bool,
) -> Option<(Matrix, f32)> {
    let occluder_height = compute_z(
        path_bounds.center_x(),
        path_bounds.center_y(),
        z_plane_params,
    );

    if !ctm.has_perspective() || directional {
        let (radius, scale, translate) = if directional {
            get_directional_params(
                occluder_height,
                light_pos.x,
                light_pos.y,
                light_pos.z,
                light_radius,
            )
        } else {
            get_spot_params(
                occluder_height,
                light_pos.x,
                light_pos.y,
                light_pos.z,
                light_radius,
            )
        };
        let mut shadow_transform = Matrix::scale_translate(scale, scale, translate.x, translate.y);
        shadow_transform.pre_concat(ctm);
        Some((shadow_transform, radius))
    } else {
        if scalar_nearly_zero(path_bounds.width(), SK_SCALAR_NEARLY_ZERO)
            || scalar_nearly_zero(path_bounds.height(), SK_SCALAR_NEARLY_ZERO)
        {
            return None;
        }

        let quad = path_bounds.to_quad();
        let mut pts = [Point::new(0.0, 0.0); 4];
        ctm.map_points(&mut pts, &quad);

        let mut pts3d = [
            Point3::new(
                pts[0].x,
                pts[0].y,
                compute_z(path_bounds.left, path_bounds.top, z_plane_params),
            ),
            Point3::new(
                pts[1].x,
                pts[1].y,
                compute_z(path_bounds.right, path_bounds.top, z_plane_params),
            ),
            Point3::new(
                pts[2].x,
                pts[2].y,
                compute_z(path_bounds.right, path_bounds.bottom, z_plane_params),
            ),
            Point3::new(
                pts[3].x,
                pts[3].y,
                compute_z(path_bounds.left, path_bounds.bottom, z_plane_params),
            ),
        ];

        // Project from light through corners to z = 0 plane.
        for p in &mut pts3d {
            let dz = light_pos.z - p.z;
            if dz <= SK_SCALAR_NEARLY_ZERO {
                return None;
            }
            let z_ratio = p.z / dz;
            p.x -= (light_pos.x - p.x) * z_ratio;
            p.y -= (light_pos.y - p.y) * z_ratio;
            p.z = 1.0;
        }

        let mut h0 = pts3d[1].cross(pts3d[0]).cross(pts3d[2].cross(pts3d[3]));
        let mut h1 = pts3d[0].cross(pts3d[3]).cross(pts3d[1].cross(pts3d[2]));
        let h2 = pts3d[0].cross(pts3d[2]).cross(pts3d[1].cross(pts3d[3]));

        if scalar_nearly_zero(h2.z, SK_SCALAR_NEARLY_ZERO) {
            return None;
        }

        // Exact C++ orientation adjustment (note: w is reused as in SkDrawShadowInfo.cpp).
        let v0 = Point3::new(
            pts3d[3].x - pts3d[0].x,
            pts3d[3].y - pts3d[0].y,
            pts3d[3].z - pts3d[0].z,
        );
        let w = Point3::new(
            h0.x - pts3d[0].x,
            h0.y - pts3d[0].y,
            h0.z - pts3d[0].z,
        );
        let perp_dot0 = v0.x * w.y - v0.y * w.x;
        if perp_dot0 > 0.0 {
            h0 = Point3::new(-h0.x, -h0.y, -h0.z);
        }
        let v1 = Point3::new(
            pts3d[1].x - pts3d[0].x,
            pts3d[1].y - pts3d[0].y,
            pts3d[1].z - pts3d[0].z,
        );
        let perp_dot1 = v1.x * w.y - v1.y * w.x;
        if perp_dot1 < 0.0 {
            h1 = Point3::new(-h1.x, -h1.y, -h1.z);
        }

        let mut shadow_transform = Matrix::make_all(
            h0.x / h2.z,
            h1.x / h2.z,
            h2.x / h2.z,
            h0.y / h2.z,
            h1.y / h2.z,
            h2.y / h2.z,
            h0.z / h2.z,
            h1.z / h2.z,
            1.0,
        );

        let x_scale = 2.0 / (path_bounds.right - path_bounds.left);
        let y_scale = 2.0 / (path_bounds.bottom - path_bounds.top);
        let to_homogeneous = Matrix::make_all(
            x_scale,
            0.0,
            -x_scale * path_bounds.left - 1.0,
            0.0,
            y_scale,
            -y_scale * path_bounds.top - 1.0,
            0.0,
            0.0,
            1.0,
        );
        shadow_transform.pre_concat(&to_homogeneous);

        let radius = spot_blur_radius(occluder_height, light_pos.z, light_radius);
        Some((shadow_transform, radius))
    }
}

/// Exact port of `SkDrawShadowMetrics::GetLocalBounds`.
pub fn get_local_bounds(path_bounds: Rect, rec: &DrawShadowRec, ctm: &Matrix) -> Rect {
    let mut ambient_bounds = path_bounds;
    let occluder_z = if scalar_nearly_zero(rec.z_plane_params.x, SK_SCALAR_NEARLY_ZERO)
        && scalar_nearly_zero(rec.z_plane_params.y, SK_SCALAR_NEARLY_ZERO)
    {
        rec.z_plane_params.z
    } else {
        let mut z = compute_z(ambient_bounds.left, ambient_bounds.top, rec.z_plane_params);
        z = cpp_max_f32(
            z,
            compute_z(ambient_bounds.right, ambient_bounds.top, rec.z_plane_params),
        );
        z = cpp_max_f32(
            z,
            compute_z(ambient_bounds.left, ambient_bounds.bottom, rec.z_plane_params),
        );
        cpp_max_f32(
            z,
            compute_z(ambient_bounds.right, ambient_bounds.bottom, rec.z_plane_params),
        )
    };

    let directional = (rec.flags & SHADOW_FLAG_DIRECTIONAL_LIGHT) != 0;
    let ambient_blur;
    let mut spot_blur;
    let spot_scale;
    let spot_offset;

    if ctm.has_perspective() {
        ambient_bounds = matrix_map_rect(ctm, ambient_bounds);
        ambient_blur = ambient_blur_radius(occluder_z);

        if directional {
            let (b, s, o) = get_directional_params(
                occluder_z,
                rec.light_pos.x,
                rec.light_pos.y,
                rec.light_pos.z,
                rec.light_radius,
            );
            spot_blur = b;
            spot_scale = s;
            spot_offset = o;
        } else {
            let dev_light_pos = matrix_map_point(ctm, Point::new(rec.light_pos.x, rec.light_pos.y));
            let (b, s, o) = get_spot_params(
                occluder_z,
                dev_light_pos.x,
                dev_light_pos.y,
                rec.light_pos.z,
                rec.light_radius,
            );
            spot_blur = b;
            spot_scale = s;
            spot_offset = o;
        }
    } else {
        let dev_to_src_scale = 1.0_f32 / ctm.get_min_scale();
        let dev_space_ambient_blur = ambient_blur_radius(occluder_z);
        ambient_blur = dev_space_ambient_blur * dev_to_src_scale;

        if directional {
            let (b, s, mut o) = get_directional_params(
                occluder_z,
                rec.light_pos.x,
                rec.light_pos.y,
                rec.light_pos.z,
                rec.light_radius,
            );
            if let Some(inv) = ctm.invert() {
                o = matrix_map_vector(&inv, o);
            }
            spot_blur = b;
            spot_scale = s;
            spot_offset = o;
        } else {
            let (b, s, o) = get_spot_params(
                occluder_z,
                rec.light_pos.x,
                rec.light_pos.y,
                rec.light_pos.z,
                rec.light_radius,
            );
            spot_blur = b;
            spot_scale = s;
            spot_offset = o;
        }

        spot_blur *= dev_to_src_scale;
    }

    let mut spot_bounds = ambient_bounds;
    ambient_bounds.left -= ambient_blur;
    ambient_bounds.top -= ambient_blur;
    ambient_bounds.right += ambient_blur;
    ambient_bounds.bottom += ambient_blur;

    spot_bounds.left = spot_bounds.left * spot_scale + spot_offset.x - spot_blur;
    spot_bounds.top = spot_bounds.top * spot_scale + spot_offset.y - spot_blur;
    spot_bounds.right = spot_bounds.right * spot_scale + spot_offset.x + spot_blur;
    spot_bounds.bottom = spot_bounds.bottom * spot_scale + spot_offset.y + spot_blur;

    let mut bounds = ambient_bounds;
    bounds.join(spot_bounds);
    bounds.left -= 1.0;
    bounds.top -= 1.0;
    bounds.right += 1.0;
    bounds.bottom += 1.0;

    if ctm.has_perspective() {
        if let Some(inv) = ctm.invert() {
            bounds = matrix_map_rect(&inv, bounds);
        }
    }

    bounds
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ambient_and_spot_metrics() {
        assert_eq!(ambient_blur_radius(10.0), 5.0);
        assert_eq!(ambient_blur_radius(500.0), K_MAX_AMBIENT_RADIUS);
        assert!((ambient_recip_alpha(128.0) - 2.0).abs() < 1e-6);

        let (blur, scale, trans) = get_spot_params(10.0, 100.0, 200.0, 110.0, 50.0);
        assert!((blur - 5.0).abs() < 1e-5);
        assert!((scale - 1.1).abs() < 1e-5);
        assert!((trans.x + 10.0).abs() < 1e-5);
        assert!((trans.y + 20.0).abs() < 1e-5);
    }

    #[test]
    fn test_get_spot_shadow_transform_affine_and_directional() {
        let (mat, radius) = get_spot_shadow_transform(
            Point3::new(0.0, 0.0, 100.0),
            20.0,
            &Matrix::identity(),
            Point3::new(0.0, 0.0, 20.0),
            Rect::make_ltrb(0.0, 0.0, 50.0, 50.0),
            false,
        )
        .unwrap();
        assert!((radius - 5.0).abs() < 1e-5);
        assert!((mat.mat[0] - 1.25).abs() < 1e-5);

        let (dmat, dradius) = get_spot_shadow_transform(
            Point3::new(0.0, -1.0, 1.0),
            0.5,
            &Matrix::identity(),
            Point3::new(0.0, 0.0, 10.0),
            Rect::make_ltrb(0.0, 0.0, 50.0, 50.0),
            true,
        )
        .unwrap();
        assert!((dradius - 5.0).abs() < 1e-5);
        assert!((dmat.mat[5] - 10.0).abs() < 1e-5);
    }

    #[test]
    fn test_get_local_bounds_basic() {
        let rec = DrawShadowRec {
            z_plane_params: Point3::new(0.0, 0.0, 10.0),
            light_pos: Point3::new(50.0, 50.0, 600.0),
            light_radius: 100.0,
            ambient_color: 0xFF000000,
            spot_color: 0xFF000000,
            flags: 0,
        };
        let b = get_local_bounds(
            Rect::make_ltrb(0.0, 0.0, 100.0, 100.0),
            &rec,
            &Matrix::identity(),
        );
        assert!(b.left < 0.0 && b.top < 0.0 && b.right > 100.0 && b.bottom > 100.0);
    }
}
