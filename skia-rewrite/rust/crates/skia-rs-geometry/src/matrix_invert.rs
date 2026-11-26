#![forbid(unsafe_code)]

use crate::all_finite;

pub fn invert_2x2_matrix(in_matrix: &[f32; 4], out_matrix: Option<&mut [f32; 4]>) -> f32 {
    let a00 = in_matrix[0] as f64;
    let a01 = in_matrix[1] as f64;
    let a10 = in_matrix[2] as f64;
    let a11 = in_matrix[3] as f64;

    let mut determinant = a00 * a11 - a01 * a10;
    if let Some(out) = out_matrix {
        let invdet = 1.0_f64 / determinant;
        out[0] = (a11 * invdet) as f32;
        out[1] = (-a01 * invdet) as f32;
        out[2] = (-a10 * invdet) as f32;
        out[3] = (a00 * invdet) as f32;
        if !all_finite(out) {
            determinant = 0.0;
        }
    }
    determinant as f32
}

pub fn invert_3x3_matrix(in_matrix: &[f32; 9], out_matrix: Option<&mut [f32; 9]>) -> f32 {
    let a00 = in_matrix[0] as f64;
    let a01 = in_matrix[1] as f64;
    let a02 = in_matrix[2] as f64;
    let a10 = in_matrix[3] as f64;
    let a11 = in_matrix[4] as f64;
    let a12 = in_matrix[5] as f64;
    let a20 = in_matrix[6] as f64;
    let a21 = in_matrix[7] as f64;
    let a22 = in_matrix[8] as f64;

    let b01 = a22 * a11 - a12 * a21;
    let b11 = -a22 * a10 + a12 * a20;
    let b21 = a21 * a10 - a11 * a20;

    let mut determinant = a00 * b01 + a01 * b11 + a02 * b21;
    if let Some(out) = out_matrix {
        let invdet = 1.0_f64 / determinant;
        out[0] = (b01 * invdet) as f32;
        out[1] = ((-a22 * a01 + a02 * a21) * invdet) as f32;
        out[2] = ((a12 * a01 - a02 * a11) * invdet) as f32;
        out[3] = (b11 * invdet) as f32;
        out[4] = ((a22 * a00 - a02 * a20) * invdet) as f32;
        out[5] = ((-a12 * a00 + a02 * a10) * invdet) as f32;
        out[6] = (b21 * invdet) as f32;
        out[7] = ((-a21 * a00 + a01 * a20) * invdet) as f32;
        out[8] = ((a11 * a00 - a01 * a10) * invdet) as f32;
        if !all_finite(out) {
            determinant = 0.0;
        }
    }
    determinant as f32
}

pub fn invert_4x4_matrix(in_matrix: &[f32; 16], out_matrix: Option<&mut [f32; 16]>) -> f32 {
    let a00 = in_matrix[0] as f64;
    let a01 = in_matrix[1] as f64;
    let a02 = in_matrix[2] as f64;
    let a03 = in_matrix[3] as f64;
    let a10 = in_matrix[4] as f64;
    let a11 = in_matrix[5] as f64;
    let a12 = in_matrix[6] as f64;
    let a13 = in_matrix[7] as f64;
    let a20 = in_matrix[8] as f64;
    let a21 = in_matrix[9] as f64;
    let a22 = in_matrix[10] as f64;
    let a23 = in_matrix[11] as f64;
    let a30 = in_matrix[12] as f64;
    let a31 = in_matrix[13] as f64;
    let a32 = in_matrix[14] as f64;
    let a33 = in_matrix[15] as f64;

    let mut b00 = a00 * a11 - a01 * a10;
    let mut b01 = a00 * a12 - a02 * a10;
    let mut b02 = a00 * a13 - a03 * a10;
    let mut b03 = a01 * a12 - a02 * a11;
    let mut b04 = a01 * a13 - a03 * a11;
    let mut b05 = a02 * a13 - a03 * a12;
    let mut b06 = a20 * a31 - a21 * a30;
    let mut b07 = a20 * a32 - a22 * a30;
    let mut b08 = a20 * a33 - a23 * a30;
    let mut b09 = a21 * a32 - a22 * a31;
    let mut b10 = a21 * a33 - a23 * a31;
    let mut b11 = a22 * a33 - a23 * a32;

    let mut determinant =
        b00 * b11 - b01 * b10 + b02 * b09 + b03 * b08 - b04 * b07 + b05 * b06;
    if let Some(out) = out_matrix {
        let invdet = 1.0_f64 / determinant;
        b00 *= invdet;
        b01 *= invdet;
        b02 *= invdet;
        b03 *= invdet;
        b04 *= invdet;
        b05 *= invdet;
        b06 *= invdet;
        b07 *= invdet;
        b08 *= invdet;
        b09 *= invdet;
        b10 *= invdet;
        b11 *= invdet;

        out[0] = (a11 * b11 - a12 * b10 + a13 * b09) as f32;
        out[1] = (a02 * b10 - a01 * b11 - a03 * b09) as f32;
        out[2] = (a31 * b05 - a32 * b04 + a33 * b03) as f32;
        out[3] = (a22 * b04 - a21 * b05 - a23 * b03) as f32;
        out[4] = (a12 * b08 - a10 * b11 - a13 * b07) as f32;
        out[5] = (a00 * b11 - a02 * b08 + a03 * b07) as f32;
        out[6] = (a32 * b02 - a30 * b05 - a33 * b01) as f32;
        out[7] = (a20 * b05 - a22 * b02 + a23 * b01) as f32;
        out[8] = (a10 * b10 - a11 * b08 + a13 * b06) as f32;
        out[9] = (a01 * b08 - a00 * b10 - a03 * b06) as f32;
        out[10] = (a30 * b04 - a31 * b02 + a33 * b00) as f32;
        out[11] = (a21 * b02 - a20 * b04 - a23 * b00) as f32;
        out[12] = (a11 * b07 - a10 * b09 - a12 * b06) as f32;
        out[13] = (a00 * b09 - a01 * b07 + a02 * b06) as f32;
        out[14] = (a31 * b01 - a30 * b03 - a32 * b00) as f32;
        out[15] = (a20 * b03 - a21 * b01 + a22 * b00) as f32;

        if !all_finite(out) {
            determinant = 0.0;
        }
    }
    determinant as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invert_matrices() {
        let m2 = [4.0, 7.0, 2.0, 6.0];
        let mut inv2 = [0.0; 4];
        let det2 = invert_2x2_matrix(&m2, Some(&mut inv2));
        assert_eq!(det2, 10.0);
        assert!((inv2[0] - 0.6).abs() < 1e-6);
        assert!((inv2[1] + 0.7).abs() < 1e-6);

        let m3 = [1.0, 0.0, 2.0, 0.0, 1.0, 3.0, 0.0, 0.0, 1.0];
        let mut inv3 = [0.0; 9];
        assert_eq!(invert_3x3_matrix(&m3, Some(&mut inv3)), 1.0);
        assert_eq!(inv3[2], -2.0);
        assert_eq!(inv3[5], -3.0);

        let m4 = [
            2.0, 0.0, 0.0, 0.0,
            0.0, 4.0, 0.0, 0.0,
            0.0, 0.0, 5.0, 0.0,
            1.0, 2.0, 3.0, 1.0,
        ];
        let mut inv4 = [0.0; 16];
        assert_eq!(invert_4x4_matrix(&m4, Some(&mut inv4)), 40.0);
        assert!((inv4[0] - 0.5).abs() < 1e-6);
        assert!((inv4[12] + 0.5).abs() < 1e-6);
    }
}
