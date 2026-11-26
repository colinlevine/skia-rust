//! Decimal float formatting matching Skia's `SkFloatToDecimal`.

pub const MAX_FLOAT_TO_DECIMAL_LEN: usize = 49;

fn pow_by_squaring(mut value: f64, mut base: f64, mut e: i32) -> f64 {
    debug_assert!(e > 0);
    loop {
        if (e & 1) != 0 {
            value *= base;
        }
        e >>= 1;
        if e == 0 {
            return value;
        }
        base *= base;
    }
}

fn pow10(e: i32) -> f64 {
    match e {
        0 => 1.0,
        1 => 10.0,
        2 => 100.0,
        3 => 1e+03,
        4 => 1e+04,
        5 => 1e+05,
        6 => 1e+06,
        7 => 1e+07,
        8 => 1e+08,
        9 => 1e+09,
        10 => 1e+10,
        11 => 1e+11,
        12 => 1e+12,
        13 => 1e+13,
        14 => 1e+14,
        15 => 1e+15,
        _ if e > 15 => pow_by_squaring(1e+15, 10.0, e - 15),
        _ => pow_by_squaring(1.0, 0.1, -e),
    }
}

/// Extract base-2 exponent matching `std::frexp(float, &exp)` for positive finite `value`.
fn frexp_exp_f32(value: f32) -> i32 {
    let bits = value.to_bits();
    let exp = ((bits >> 23) & 0xFF) as i32;
    if exp != 0 {
        exp - 126
    } else {
        // Subnormal: normalize by multiplying by 2^24.
        let scaled = value * ((1u32 << 24) as f32);
        let scaled_exp = ((scaled.to_bits() >> 23) & 0xFF) as i32;
        scaled_exp - 126 - 24
    }
}

pub fn float_to_decimal(mut value: f32, output: &mut [u8; MAX_FLOAT_TO_DECIMAL_LEN]) -> u32 {
    let mut out_idx = 0usize;
    let end = MAX_FLOAT_TO_DECIMAL_LEN - 1;

    if value == f32::INFINITY {
        value = f32::MAX;
    }
    if value == f32::NEG_INFINITY {
        value = -f32::MAX;
    }
    if !value.is_finite() || value == 0.0f32 {
        output[0] = b'0';
        output[1] = 0;
        return 1;
    }
    if value < 0.0f32 {
        output[out_idx] = b'-';
        out_idx += 1;
        value = -value;
    }

    let binary_exponent = frexp_exp_f32(value);
    const LOG2: f64 = 0.3010299956639812;
    let decimal_exponent = (LOG2 * (binary_exponent as f64)).floor() as i32;
    let mut decimal_shift = decimal_exponent - 8;
    let power = pow10(-decimal_shift);
    let mut d = ((value as f64) * power + 0.5) as i32;
    if d > 167772159 {
        decimal_shift = decimal_exponent - 7;
        d = ((value as f64) * (power * 0.1) + 0.5) as i32;
    }
    while d % 10 == 0 {
        d /= 10;
        decimal_shift += 1;
    }

    let mut buffer = [0u8; 9];
    let mut buf_idx = 0usize;
    loop {
        buffer[buf_idx] = (d % 10) as u8;
        buf_idx += 1;
        d /= 10;
        if d == 0 {
            break;
        }
    }

    if decimal_shift >= 0 {
        while buf_idx > 0 {
            buf_idx -= 1;
            output[out_idx] = b'0' + buffer[buf_idx];
            out_idx += 1;
        }
        for _ in 0..decimal_shift {
            output[out_idx] = b'0';
            out_idx += 1;
        }
    } else {
        let places_before = (buf_idx as i32) + decimal_shift;
        if places_before > 0 {
            for _ in 0..places_before {
                buf_idx -= 1;
                output[out_idx] = b'0' + buffer[buf_idx];
                out_idx += 1;
            }
            output[out_idx] = b'.';
            out_idx += 1;
        } else {
            output[out_idx] = b'.';
            out_idx += 1;
            for _ in 0..(-places_before) {
                output[out_idx] = b'0';
                out_idx += 1;
            }
        }
        while buf_idx > 0 {
            buf_idx -= 1;
            output[out_idx] = b'0' + buffer[buf_idx];
            out_idx += 1;
            if out_idx == end {
                break;
            }
        }
    }

    output[out_idx] = 0;
    out_idx as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(v: f32) -> String {
        let mut buf = [0u8; MAX_FLOAT_TO_DECIMAL_LEN];
        let len = float_to_decimal(v, &mut buf) as usize;
        assert_eq!(buf[len], 0);
        std::str::from_utf8(&buf[..len]).unwrap().to_string()
    }

    #[test]
    fn basic_values() {
        assert_eq!(fmt(0.0), "0");
        assert_eq!(fmt(-0.0), "0");
        assert_eq!(fmt(f32::NAN), "0");
        assert_eq!(fmt(1.0), "1");
        assert_eq!(fmt(-1.5), "-1.5");
        assert_eq!(fmt(0.25), ".25");
        assert_eq!(fmt(f32::MIN_POSITIVE).len(), 47);
        assert_eq!(fmt(-f32::MIN_POSITIVE).len(), 48);
    }
}
