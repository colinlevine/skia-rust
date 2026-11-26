#![forbid(unsafe_code)]

/// Pure safe Rust vertex index iterator matching `VertState`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct VertStep {
    pub f0: i32,
    pub f1: i32,
    pub f2: i32,
    pub next_index: i32,
}

pub fn step_triangles(curr_index: i32, count: i32) -> Option<VertStep> {
    if curr_index + 3 > count {
        return None;
    }
    Some(VertStep {
        f0: curr_index,
        f1: curr_index + 1,
        f2: curr_index + 2,
        next_index: curr_index + 3,
    })
}

pub fn step_triangles_indexed(curr_index: i32, indices: &[u16]) -> Option<VertStep> {
    if curr_index < 0 || (curr_index as usize) + 3 > indices.len() {
        return None;
    }
    let idx = curr_index as usize;
    Some(VertStep {
        f0: indices[idx] as i32,
        f1: indices[idx + 1] as i32,
        f2: indices[idx + 2] as i32,
        next_index: curr_index + 3,
    })
}

pub fn step_triangle_strip(curr_index: i32, count: i32) -> Option<VertStep> {
    if curr_index + 3 > count {
        return None;
    }
    let (f0, f1) = if (curr_index & 1) != 0 {
        (curr_index + 1, curr_index)
    } else {
        (curr_index, curr_index + 1)
    };
    Some(VertStep {
        f0,
        f1,
        f2: curr_index + 2,
        next_index: curr_index + 1,
    })
}

pub fn step_triangle_strip_indexed(curr_index: i32, indices: &[u16]) -> Option<VertStep> {
    if curr_index < 0 || (curr_index as usize) + 3 > indices.len() {
        return None;
    }
    let idx = curr_index as usize;
    let (f0, f1) = if (curr_index & 1) != 0 {
        (indices[idx + 1] as i32, indices[idx] as i32)
    } else {
        (indices[idx] as i32, indices[idx + 1] as i32)
    };
    Some(VertStep {
        f0,
        f1,
        f2: indices[idx + 2] as i32,
        next_index: curr_index + 1,
    })
}

pub fn step_triangle_fan(curr_index: i32, count: i32) -> Option<VertStep> {
    if curr_index + 3 > count {
        return None;
    }
    Some(VertStep {
        f0: 0,
        f1: curr_index + 1,
        f2: curr_index + 2,
        next_index: curr_index + 1,
    })
}

pub fn step_triangle_fan_indexed(curr_index: i32, indices: &[u16]) -> Option<VertStep> {
    if curr_index < 0 || (curr_index as usize) + 3 > indices.len() {
        return None;
    }
    let idx = curr_index as usize;
    Some(VertStep {
        f0: indices[0] as i32,
        f1: indices[idx + 1] as i32,
        f2: indices[idx + 2] as i32,
        next_index: curr_index + 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vert_steps() {
        let s = step_triangles(0, 6).unwrap();
        assert_eq!((s.f0, s.f1, s.f2, s.next_index), (0, 1, 2, 3));
        assert!(step_triangles(4, 6).is_none());

        let s1 = step_triangle_strip(1, 5).unwrap();
        assert_eq!((s1.f0, s1.f1, s1.f2, s1.next_index), (2, 1, 3, 2));

        let idx = [10u16, 20, 30, 40];
        let sf = step_triangle_fan_indexed(1, &idx).unwrap();
        assert_eq!((sf.f0, sf.f1, sf.f2, sf.next_index), (10, 30, 40, 2));
    }
}
