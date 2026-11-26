#![forbid(unsafe_code)]

use skia_rs_geometry::{Point, Rect};
use skia_rs_path::analysis::contains;
use skia_rs_path::builder::PathBuilder;
use skia_rs_path::types::{Direction, FillType, PathDataBuf, Verb};

use crate::arena::OpState;
use crate::conic::DConic;
use crate::cubic::DCubic;
use crate::curve::{ReduceOrder, ReduceQuadratics};
use crate::intersections::Intersections;
use crate::line::DLine;
use crate::quad::DQuad;
use crate::types::{almost_equal_ulps_f32, DPoint};
use crate::{as_winding, compute_tight_bounds, op, simplify, OpBuilder, PathOp};

fn rect_path(l: f32, t: f32, r: f32, b: f32, dir: Direction, ft: FillType) -> PathDataBuf {
    let mut pb = PathBuilder::new(ft);
    pb.add_rect(Rect::new(l, t, r, b), dir, 0);
    pb.into_buf()
}

fn circle_path(cx: f32, cy: f32, r: f32, dir: Direction, ft: FillType) -> PathDataBuf {
    let mut pb = PathBuilder::new(ft);
    pb.add_circle(Point::new(cx, cy), r, dir);
    pb.into_buf()
}

fn path_contains(path: &PathDataBuf, x: f32, y: f32) -> bool {
    let raw = path.as_raw(false).expect("finite path");
    contains(&raw, Point::new(x, y))
}

#[test]
fn test_rect_ops_all_five() {
    let a = rect_path(0.0, 0.0, 100.0, 100.0, Direction::Cw, FillType::Winding);
    let b = rect_path(50.0, 50.0, 150.0, 150.0, Direction::Cw, FillType::Winding);

    // Intersect: [50, 50, 100, 100]
    let sect = op(&a, &b, PathOp::Intersect).expect("intersect");
    assert!(path_contains(&sect, 75.0, 75.0));
    assert!(!path_contains(&sect, 25.0, 25.0));
    assert!(!path_contains(&sect, 125.0, 125.0));

    // Union: covers (25,25), (75,75), (125,125), not (25,125) or (125,25)
    let uni = op(&a, &b, PathOp::Union).expect("union");
    assert!(path_contains(&uni, 25.0, 25.0));
    assert!(path_contains(&uni, 75.0, 75.0));
    assert!(path_contains(&uni, 125.0, 125.0));
    assert!(!path_contains(&uni, 25.0, 125.0));
    assert!(!path_contains(&uni, 125.0, 25.0));

    // Difference: a - b
    let diff = op(&a, &b, PathOp::Difference).expect("diff");
    assert!(path_contains(&diff, 25.0, 25.0));
    assert!(path_contains(&diff, 75.0, 25.0));
    assert!(path_contains(&diff, 25.0, 75.0));
    assert!(!path_contains(&diff, 75.0, 75.0));
    assert!(!path_contains(&diff, 125.0, 125.0));

    // ReverseDifference: b - a
    let rdiff = op(&a, &b, PathOp::ReverseDifference).expect("rdiff");
    assert!(!path_contains(&rdiff, 25.0, 25.0));
    assert!(!path_contains(&rdiff, 75.0, 75.0));
    assert!(path_contains(&rdiff, 125.0, 125.0));
    assert!(path_contains(&rdiff, 75.0, 125.0));
    assert!(path_contains(&rdiff, 125.0, 75.0));

    // Xor: (a | b) - (a & b)
    let xor = op(&a, &b, PathOp::Xor).expect("xor");
    assert!(path_contains(&xor, 25.0, 25.0));
    assert!(!path_contains(&xor, 75.0, 75.0));
    assert!(path_contains(&xor, 125.0, 125.0));
}

#[test]
fn test_rect_coincident_and_identical() {
    let a = rect_path(0.0, 0.0, 100.0, 100.0, Direction::Cw, FillType::Winding);
    let b = rect_path(0.0, 0.0, 100.0, 100.0, Direction::Cw, FillType::Winding);
    let c = rect_path(0.0, 0.0, 100.0, 100.0, Direction::Ccw, FillType::Winding);

    // Identical difference -> empty
    let d1 = op(&a, &b, PathOp::Difference).expect("identical diff");
    assert!(d1.is_empty());
    let d2 = op(&a, &c, PathOp::Difference).expect("opposite winding diff");
    assert!(d2.is_empty());

    // Identical xor -> empty
    let x1 = op(&a, &b, PathOp::Xor).expect("identical xor");
    assert!(x1.is_empty());

    // Identical union -> same area
    let u1 = op(&a, &b, PathOp::Union).expect("identical union");
    assert!(path_contains(&u1, 50.0, 50.0));
    assert!(!path_contains(&u1, -1.0, 50.0));

    // Shared edge union: [0,0,50,100] | [50,0,100,100] = [0,0,100,100]
    let left = rect_path(0.0, 0.0, 50.0, 100.0, Direction::Cw, FillType::Winding);
    let right = rect_path(50.0, 0.0, 100.0, 100.0, Direction::Cw, FillType::Winding);
    let u2 = op(&left, &right, PathOp::Union).expect("shared edge union");
    assert!(path_contains(&u2, 25.0, 50.0));
    assert!(path_contains(&u2, 50.0, 50.0));
    assert!(path_contains(&u2, 75.0, 50.0));
    assert!(!path_contains(&u2, 105.0, 50.0));
}

#[test]
fn test_inverse_fill_type_ops() {
    let a = rect_path(
        0.0,
        0.0,
        100.0,
        100.0,
        Direction::Cw,
        FillType::InverseWinding,
    );
    let b = rect_path(50.0, 50.0, 150.0, 150.0, Direction::Cw, FillType::Winding);
    let res = op(&a, &b, PathOp::Intersect).expect("inverse intersect");
    assert!(!res.fill_type.is_inverse());
    assert!(!path_contains(&res, 75.0, 75.0));
    assert!(path_contains(&res, 125.0, 125.0));
    assert!(!path_contains(&res, 25.0, 25.0));

    let res_u = op(&a, &b, PathOp::Union).expect("inverse union");
    assert!(res_u.fill_type.is_inverse());
    assert!(path_contains(&res_u, 200.0, 200.0));
    assert!(path_contains(&res_u, 75.0, 75.0));
    assert!(!path_contains(&res_u, 25.0, 25.0));
}

#[test]
fn test_circle_conic_ops() {
    let c1 = circle_path(50.0, 50.0, 40.0, Direction::Cw, FillType::Winding);
    let c2 = circle_path(90.0, 50.0, 40.0, Direction::Cw, FillType::Winding);

    let uni = op(&c1, &c2, PathOp::Union).expect("circle union");
    assert!(path_contains(&uni, 30.0, 50.0));
    assert!(path_contains(&uni, 70.0, 50.0));
    assert!(path_contains(&uni, 110.0, 50.0));
    assert!(!path_contains(&uni, 70.0, 5.0));

    let sect = op(&c1, &c2, PathOp::Intersect).expect("circle intersect");
    assert!(!path_contains(&sect, 20.0, 50.0));
    assert!(path_contains(&sect, 70.0, 50.0));
    assert!(!path_contains(&sect, 120.0, 50.0));

    let diff = op(&c1, &c2, PathOp::Difference).expect("circle diff");
    assert!(path_contains(&diff, 30.0, 50.0));
    assert!(!path_contains(&diff, 70.0, 50.0));
    assert!(!path_contains(&diff, 110.0, 50.0));
}

#[test]
fn test_quad_and_cubic_ops() {
    let mut pb1 = PathBuilder::new(FillType::Winding);
    pb1.move_to(Point::new(0.0, 50.0));
    pb1.quad_to(Point::new(50.0, -30.0), Point::new(100.0, 50.0));
    pb1.quad_to(Point::new(50.0, 130.0), Point::new(0.0, 50.0));
    pb1.close();
    let p1 = pb1.into_buf();

    let mut pb2 = PathBuilder::new(FillType::Winding);
    pb2.move_to(Point::new(30.0, 10.0));
    pb2.cubic_to(
        Point::new(80.0, -10.0),
        Point::new(120.0, 110.0),
        Point::new(130.0, 50.0),
    );
    pb2.line_to(Point::new(130.0, 100.0));
    pb2.line_to(Point::new(30.0, 100.0));
    pb2.close();
    let p2 = pb2.into_buf();

    let uni = op(&p1, &p2, PathOp::Union).expect("quad-cubic union");
    assert!(!uni.is_empty());
    assert!(path_contains(&uni, 50.0, 50.0));
    assert!(path_contains(&uni, 110.0, 90.0));

    let sect = op(&p1, &p2, PathOp::Intersect).expect("quad-cubic intersect");
    assert!(!sect.is_empty());
    assert!(path_contains(&sect, 50.0, 50.0));
    assert!(!path_contains(&sect, 10.0, 50.0));
}

#[test]
fn test_simplify_self_intersecting_and_star() {
    // Figure-8 bowtie
    let mut pb = PathBuilder::new(FillType::Winding);
    pb.move_to(Point::new(0.0, 0.0));
    pb.line_to(Point::new(100.0, 100.0));
    pb.line_to(Point::new(100.0, 0.0));
    pb.line_to(Point::new(0.0, 100.0));
    pb.close();
    let bowtie = pb.into_buf();
    let s = simplify(&bowtie).expect("simplify bowtie");
    assert!(path_contains(&s, 20.0, 50.0));
    assert!(path_contains(&s, 80.0, 50.0));
    assert!(!path_contains(&s, 50.0, 10.0));
    assert!(!path_contains(&s, 50.0, 90.0));

    // 5-point star: Winding fills center, EvenOdd leaves center hollow
    let mut pts = [Point::ZERO; 5];
    for (i, p) in pts.iter_mut().enumerate() {
        let angle = (i as f32) * 4.0 * core::f32::consts::PI / 5.0 - core::f32::consts::FRAC_PI_2;
        *p = Point::new(100.0 + 80.0 * angle.cos(), 100.0 + 80.0 * angle.sin());
    }
    let mut star_w = PathBuilder::new(FillType::Winding);
    star_w.move_to(pts[0]);
    for &p in &pts[1..] {
        star_w.line_to(p);
    }
    star_w.close();
    let sw = simplify(&star_w.into_buf()).expect("simplify star winding");
    assert!(path_contains(&sw, 100.0, 100.0));

    let mut star_eo = PathBuilder::new(FillType::EvenOdd);
    star_eo.move_to(pts[0]);
    for &p in &pts[1..] {
        star_eo.line_to(p);
    }
    star_eo.close();
    let seo = simplify(&star_eo.into_buf()).expect("simplify star evenodd");
    assert!(!path_contains(&seo, 100.0, 100.0));
}

#[test]
fn test_as_winding_nested_rects() {
    let mut pb = PathBuilder::new(FillType::EvenOdd);
    pb.add_rect(Rect::new(0.0, 0.0, 100.0, 100.0), Direction::Cw, 0);
    pb.add_rect(Rect::new(20.0, 20.0, 80.0, 80.0), Direction::Cw, 0);
    pb.add_rect(Rect::new(40.0, 40.0, 60.0, 60.0), Direction::Cw, 0);
    let eo = pb.into_buf();

    let wound = as_winding(&eo).expect("as_winding");
    assert_eq!(wound.fill_type, FillType::Winding);
    // Outer ring filled
    assert!(path_contains(&wound, 10.0, 50.0));
    // Middle ring hollow
    assert!(!path_contains(&wound, 30.0, 50.0));
    // Innermost square filled
    assert!(path_contains(&wound, 50.0, 50.0));
    // Outside empty
    assert!(!path_contains(&wound, -10.0, 50.0));
}

#[test]
fn test_compute_tight_bounds() {
    // Quad with control point far outside endpoints: (0,0), (50, 100), (100, 0)
    // Peak Y at t=0.5 is 0.25*0 + 0.5*100 + 0.25*0 = 50.
    let mut pb = PathBuilder::new(FillType::Winding);
    pb.move_to(Point::new(0.0, 0.0));
    pb.quad_to(Point::new(50.0, 100.0), Point::new(100.0, 0.0));
    pb.close();
    let p = pb.into_buf();
    let tb = compute_tight_bounds(&p).expect("tight bounds");
    assert!((tb.left - 0.0).abs() < 1e-4);
    assert!((tb.top - 0.0).abs() < 1e-4);
    assert!((tb.right - 100.0).abs() < 1e-4);
    assert!((tb.bottom - 50.0).abs() < 1e-4);
}

#[test]
fn test_op_builder_multiple_unions_and_mixed() {
    let mut builder = OpBuilder::new();
    builder.add(
        rect_path(0.0, 0.0, 40.0, 40.0, Direction::Cw, FillType::Winding),
        PathOp::Union,
    );
    builder.add(
        rect_path(30.0, 0.0, 70.0, 40.0, Direction::Ccw, FillType::Winding),
        PathOp::Union,
    );
    builder.add(
        rect_path(60.0, 0.0, 100.0, 40.0, Direction::Cw, FillType::Winding),
        PathOp::Union,
    );
    let res = builder.resolve().expect("builder union");
    assert!(path_contains(&res, 10.0, 20.0));
    assert!(path_contains(&res, 50.0, 20.0));
    assert!(path_contains(&res, 90.0, 20.0));
    assert!(!path_contains(&res, 50.0, 50.0));

    // Mixed ops
    builder.add(
        rect_path(0.0, 0.0, 100.0, 100.0, Direction::Cw, FillType::Winding),
        PathOp::Union,
    );
    builder.add(
        rect_path(25.0, 25.0, 75.0, 75.0, Direction::Cw, FillType::Winding),
        PathOp::Difference,
    );
    let hole = builder.resolve().expect("builder hole");
    assert!(path_contains(&hole, 10.0, 50.0));
    assert!(!path_contains(&hole, 50.0, 50.0));
}

#[test]
fn test_low_level_curves_and_intersections() {
    // Line-Line intersection
    let mut i = Intersections::new();
    let l1 = DLine::new(DPoint::new(0.0, 0.0), DPoint::new(10.0, 10.0));
    let l2 = DLine::new(DPoint::new(0.0, 10.0), DPoint::new(10.0, 0.0));
    assert_eq!(i.intersect_line_line(&l1, &l2), 1);
    assert!((i.pt[0].x - 5.0).abs() < 1e-6);
    assert!((i.pt[0].y - 5.0).abs() < 1e-6);

    // Quad-Quad intersection
    let mut iq = Intersections::new();
    let q1 = DQuad::new(
        DPoint::new(0.0, 0.0),
        DPoint::new(5.0, 10.0),
        DPoint::new(10.0, 0.0),
    );
    let q2 = DQuad::new(
        DPoint::new(0.0, 5.0),
        DPoint::new(5.0, -5.0),
        DPoint::new(10.0, 5.0),
    );
    let cnt = iq.intersect_quad_quad(&q1, &q2);
    assert_eq!(cnt, 2);

    // Conic-Conic intersection
    let mut ic = Intersections::new();
    let c1 = DConic::from_points(
        [
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
        ],
        core::f32::consts::FRAC_1_SQRT_2,
    );
    let c2 = DConic::from_points(
        [
            Point::new(0.0, 10.0),
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
        ],
        core::f32::consts::FRAC_1_SQRT_2,
    );
    assert!(ic.intersect_conic_conic(&c1, &c2) >= 1);

    // Cubic convex hull and reduce order
    let cubic = DCubic::new(
        DPoint::new(0.0, 0.0),
        DPoint::new(1.0, 3.0),
        DPoint::new(2.0, 3.0),
        DPoint::new(3.0, 0.0),
    );
    let mut order = [0u8; 4];
    let hull_cnt = cubic.convex_hull(&mut order);
    assert_eq!(hull_cnt, 4);

    let line_cubic = DCubic::new(
        DPoint::new(0.0, 0.0),
        DPoint::new(1.0, 1.0),
        DPoint::new(2.0, 2.0),
        DPoint::new(3.0, 3.0),
    );
    let mut ro = ReduceOrder::default();
    assert_eq!(ro.reduce_cubic(&line_cubic, ReduceQuadratics::AllowQuadratics), 2);

    // Angle ordering smoke test
    let mut state = OpState::new();
    let c = state.contour_head;
    let s1 = state.add_line(c, [Point::new(0.0, 0.0), Point::new(10.0, 0.0)]);
    let s2 = state.add_line(c, [Point::new(0.0, 0.0), Point::new(0.0, 10.0)]);
    let a1 = state.alloc_angle();
    let a2 = state.alloc_angle();
    let h1 = state.segments[s1].head;
    let t1 = state.segments[s1].tail;
    let h2 = state.segments[s2].head;
    let t2 = state.segments[s2].tail;
    state.angle_set(a1, h1, t1);
    state.angle_set(a2, h2, t2);
    assert_eq!(state.angle_orderable(a1, a2), 0);
    assert_eq!(state.angle_orderable(a2, a1), 1);

    assert!(almost_equal_ulps_f32(1.0, 1.0));
}

#[test]
fn test_multithreaded_pathops() {
    let handles: Vec<_> = (0..4)
        .map(|tid| {
            std::thread::spawn(move || {
                for i in 0..10 {
                    let off = (tid * 10 + i) as f32;
                    let a = rect_path(
                        off,
                        off,
                        off + 50.0,
                        off + 50.0,
                        Direction::Cw,
                        FillType::Winding,
                    );
                    let b = circle_path(
                        off + 40.0,
                        off + 40.0,
                        30.0,
                        Direction::Cw,
                        FillType::Winding,
                    );
                    let u = op(&a, &b, PathOp::Union).expect("mt union");
                    assert!(!u.is_empty());
                    let s = simplify(&u).expect("mt simplify");
                    assert!(!s.is_empty());
                    let w = as_winding(&s).expect("mt as_winding");
                    assert_eq!(w.fill_type, FillType::Winding);
                }
            })
        })
        .collect();
    for h in handles {
        h.join().expect("thread join");
    }
}

#[test]
fn test_empty_and_non_finite() {
    let empty = PathDataBuf::new(FillType::Winding);
    let r = rect_path(0.0, 0.0, 10.0, 10.0, Direction::Cw, FillType::Winding);
    assert!(op(&empty, &r, PathOp::Intersect).unwrap().is_empty());
    assert!(!op(&empty, &r, PathOp::Union).unwrap().is_empty());
    assert!(op(&empty, &r, PathOp::Difference).unwrap().is_empty());
    assert!(!op(&empty, &r, PathOp::ReverseDifference).unwrap().is_empty());

    let mut bad = PathBuilder::new(FillType::Winding);
    bad.move_to(Point::new(f32::NAN, 0.0));
    bad.line_to(Point::new(10.0, 10.0));
    bad.line_to(Point::new(0.0, 10.0));
    bad.close();
    let bad_buf = bad.into_buf();
    assert!(op(&bad_buf, &r, PathOp::Union).is_none());
    assert!(as_winding(&bad_buf).is_none());
    assert_eq!(bad_buf.verbs[0], Verb::Move);
}
