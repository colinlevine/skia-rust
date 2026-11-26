#![forbid(unsafe_code)]

use crate::analysis::{
    compute_convexity, compute_first_direction, compute_tight_bounds,
    conservatively_contains_rect, contains, perspective_clip, transform_convexity,
};
use crate::builder::{
    can_interpolate, create_draw_arc_path, draw_arc_is_convex, interpolate_points, PathBuilder,
};
use crate::dash::{calc_dash_parameters, filter_dash_path, valid_dash_path};
use crate::iter::{ContourIter, EdgeIter, LegacyPathIter, PathIter, RangeIter};
use crate::measure::{
    ContourMeasureIter, PathMeasure, GET_POSITION_MATRIX_FLAG, GET_POS_AND_TAN_MATRIX_FLAG,
    GET_TANGENT_MATRIX_FLAG,
};
use crate::parse::{from_svg_string, to_svg_string, PathEncoding};
use crate::serial_dump::{
    dump_builder_to_string, dump_path_to_string, read_from_memory, write_to_memory,
    RRectSerialInfo, ReadPathResult,
};
use crate::shapes::{is_nested_fill_rects, is_rect_contour, is_simple_rect, transform_dir_and_start};
use crate::stroke::{Stroke, StrokeRec};
use crate::types::*;
use skia_rs_geometry::{Matrix, Point, RRect, Rect, Vector};

fn assert_near(a: f32, b: f32, eps: f32) {
    assert!(
        (a - b).abs() <= eps,
        "expected {a} to be within {eps} of {b} (diff = {})",
        (a - b).abs()
    );
}

#[test]
fn test_types_validation_and_bounds() {
    assert!(valid_path_data(&[], &[], &[]));
    assert!(!valid_path_data(&[Point::ZERO], &[], &[]));
    assert!(!valid_path_data(&[Point::ZERO], &[Verb::Line], &[]));

    let pts = [
        Point::new(0.0, 0.0),
        Point::new(10.0, 0.0),
        Point::new(10.0, 20.0),
    ];
    let vbs = [Verb::Move, Verb::Line, Verb::Line, Verb::Close];
    assert!(valid_path_data(&pts, &vbs, &[]));
    assert!(is_closed_single_contour(&vbs));
    assert!(is_axis_aligned(&pts));
    assert_eq!(find_last_move_to_index(&vbs, pts.len()), 0);

    let b = trimmed_bounds(&pts, &vbs).unwrap();
    assert_eq!(b, Rect::from_ltrb(0.0, 0.0, 10.0, 20.0));

    // Trailing move is excluded from trimmed_bounds
    let pts2 = [Point::new(0.0, 0.0), Point::new(5.0, 5.0), Point::new(100.0, 100.0)];
    let vbs2 = [Verb::Move, Verb::Line, Verb::Move];
    let b2 = trimmed_bounds(&pts2, &vbs2).unwrap();
    assert_eq!(b2, Rect::from_ltrb(0.0, 0.0, 5.0, 5.0));
}

#[test]
fn test_iterators_and_contours() {
    let mut pb = PathBuilder::new(FillType::Winding);
    pb.move_to(Point::new(0.0, 0.0))
        .line_to(Point::new(10.0, 0.0))
        .line_to(Point::new(10.0, 10.0))
        .close()
        .move_to(Point::new(20.0, 20.0))
        .quad_to(Point::new(30.0, 20.0), Point::new(30.0, 30.0));
    let buf = pb.into_buf();

    let mut pi = PathIter::new(&buf.points, &buf.verbs, &buf.conics);
    let mut verbs = Vec::new();
    while let Some(rec) = pi.next_rec() {
        verbs.push(rec.verb);
    }
    assert_eq!(verbs, buf.verbs);

    // ContourIter should yield 2 contours
    let mut ci = ContourIter::new(&buf.points, &buf.verbs, &buf.conics);
    let mut contours = Vec::new();
    while let Some(c) = ci.next_contour() {
        contours.push(c);
    }
    assert_eq!(contours.len(), 2);
    assert_eq!(contours[0].verbs.len(), 4);
    assert_eq!(contours[1].verbs.len(), 2);

    // RangeIter should yield 6 verbs
    let range_cnt = RangeIter::new(&buf.points, &buf.verbs, &buf.conics).count();
    assert_eq!(range_cnt, 6);

    // LegacyPathIter with force_close emits closing line for open contour
    let mut li = LegacyPathIter::new(&buf.points, &buf.verbs, &buf.conics, true);
    let mut close_count = 0;
    while let Some(rec) = li.next_rec() {
        if rec.verb == Verb::Close {
            close_count += 1;
        }
    }
    assert_eq!(close_count, 2);

    // EdgeIter should visit edges
    let raw = buf.as_raw(false).unwrap();
    let mut ei = EdgeIter::new(&raw);
    let mut edge_count = 0;
    while ei.next_edge().is_some() {
        edge_count += 1;
    }
    assert!(edge_count >= 4);
}

#[test]
fn test_shapes_rect_oval_rrect_and_detection() {
    let r = Rect::from_ltrb(10.0, 20.0, 110.0, 70.0);
    let mut pb = PathBuilder::new(FillType::Winding);
    pb.add_rect(r, Direction::Cw, 0);
    let buf = pb.into_buf();

    let rc = is_rect_contour(&buf.points, &buf.verbs, buf.segment_mask as u32, false).unwrap();
    assert_eq!(rc.rect, r);
    assert!(rc.is_closed);
    assert_eq!(rc.direction, Direction::Cw);

    let sr = is_simple_rect(
        &buf.points,
        &buf.verbs,
        &buf.conics,
        buf.segment_mask as u32,
        true,
        false,
    )
    .unwrap();
    assert_eq!(sr.rect, r);

    // Nested rects
    let inner = Rect::from_ltrb(20.0, 30.0, 90.0, 60.0);
    let mut pb2 = PathBuilder::new(FillType::EvenOdd);
    pb2.add_rect(r, Direction::Cw, 0)
        .add_rect(inner, Direction::Ccw, 0);
    let buf2 = pb2.into_buf();
    let raw2 = buf2.as_raw(false).unwrap();
    let (rects, dirs) = is_nested_fill_rects(&raw2).unwrap();
    assert_eq!(rects[0], r);
    assert_eq!(rects[1], inner);
    assert_eq!(dirs[0], Direction::Cw);
    assert_eq!(dirs[1], Direction::Ccw);

    // RRect & Oval
    let mut rr = RRect::make_empty();
    rr.set_rect_xy(r, 8.0, 8.0);
    let mut pb3 = PathBuilder::new(FillType::Winding);
    pb3.add_rrect(&rr, Direction::Cw, 0);
    let buf3 = pb3.into_buf();
    assert_eq!(buf3.is_a_type, PathIsAType::RRect);

    // transform_dir_and_start flip X reverses direction
    let mut flip_x = Matrix::identity();
    flip_x.set_scale(-1.0, 1.0);
    let (new_dir, _) = transform_dir_and_start(&flip_x, false, Direction::Cw, 0);
    assert_eq!(new_dir, Direction::Ccw);
}

#[test]
fn test_analysis_convexity_contains_tight_bounds_and_perspective() {
    let mut pb = PathBuilder::new(FillType::Winding);
    pb.add_rect(Rect::from_ltrb(0.0, 0.0, 100.0, 100.0), Direction::Cw, 0);
    let buf = pb.into_buf();
    let raw = buf.as_raw(true).unwrap();

    assert_eq!(raw.convexity, Convexity::ConvexCw);
    assert_eq!(compute_first_direction(&raw), FirstDirection::Cw);
    assert!(contains(&raw, Point::new(50.0, 50.0)));
    assert!(!contains(&raw, Point::new(-1.0, 50.0)));
    assert!(conservatively_contains_rect(
        raw.points,
        raw.verbs,
        raw.conics,
        raw.convexity,
        true,
        Rect::from_ltrb(10.0, 10.0, 90.0, 90.0)
    ));
    assert!(!conservatively_contains_rect(
        raw.points,
        raw.verbs,
        raw.conics,
        raw.convexity,
        true,
        Rect::from_ltrb(10.0, 10.0, 110.0, 90.0)
    ));

    // Transform convexity under negative scale
    let mut neg = Matrix::identity();
    neg.set_scale(-1.0, 1.0);
    assert_eq!(
        transform_convexity(&neg, &buf.points, Convexity::ConvexCw),
        Convexity::ConvexCcw
    );

    // Tight bounds of a quadratic curve should be strictly smaller than control point hull
    let mut qpb = PathBuilder::new(FillType::Winding);
    qpb.move_to(Point::new(0.0, 0.0))
        .quad_to(Point::new(50.0, 100.0), Point::new(100.0, 0.0));
    let qbuf = qpb.into_buf();
    let tb = compute_tight_bounds(&qbuf.points, &qbuf.verbs, &qbuf.conics);
    assert_near(tb.top, 0.0, 1e-4);
    assert_near(tb.bottom, 50.0, 1e-4);

    // Concave U-shape
    let mut upb = PathBuilder::new(FillType::Winding);
    upb.move_to(Point::new(0.0, 0.0))
        .line_to(Point::new(100.0, 0.0))
        .line_to(Point::new(100.0, 100.0))
        .line_to(Point::new(70.0, 100.0))
        .line_to(Point::new(70.0, 30.0))
        .line_to(Point::new(30.0, 30.0))
        .line_to(Point::new(30.0, 100.0))
        .line_to(Point::new(0.0, 100.0))
        .close();
    let ubuf = upb.into_buf();
    assert_eq!(
        compute_convexity(&ubuf.points, &ubuf.verbs, &ubuf.conics),
        Convexity::Concave
    );

    // Perspective clip when all points are in front of w=0 plane returns None (no clip needed)
    let mut persp_pos = Matrix::make_all(1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.01, 1.0);
    let _ = persp_pos.get_type_mut();
    assert!(perspective_clip(&raw, &persp_pos).is_none());

    // Perspective clip when rect crosses w=0 plane returns Some(clipped)
    let mut persp_cross = Matrix::make_all(1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -0.02, 1.0);
    let _ = persp_cross.get_type_mut();
    let clipped = perspective_clip(&raw, &persp_cross).unwrap();
    assert!(!clipped.is_empty());
}

#[test]
fn test_builder_arcs_reverse_and_interpolate() {
    let mut pb = PathBuilder::new(FillType::Winding);
    pb.move_to(Point::new(0.0, 0.0))
        .arc_to_tangent(Point::new(100.0, 0.0), Point::new(100.0, 100.0), 20.0)
        .line_to(Point::new(100.0, 100.0));
    let buf = pb.into_buf();
    assert!(buf.verbs.contains(&Verb::Conic));

    // Draw arc helper
    assert!(draw_arc_is_convex(90.0, ArcType::Arc, true));
    assert!(!draw_arc_is_convex(270.0, ArcType::Wedge, true));
    let arc_buf = create_draw_arc_path(
        Rect::from_ltrb(0.0, 0.0, 100.0, 100.0),
        0.0,
        90.0,
        ArcType::Wedge,
        true,
    );
    assert!(!arc_buf.is_empty());

    // Interpolation
    let mut pb1 = PathBuilder::new(FillType::Winding);
    pb1.move_to(Point::new(0.0, 0.0)).line_to(Point::new(10.0, 0.0));
    let b1 = pb1.into_buf();
    let mut pb2 = PathBuilder::new(FillType::Winding);
    pb2.move_to(Point::new(0.0, 10.0)).line_to(Point::new(20.0, 10.0));
    let b2 = pb2.into_buf();
    assert!(can_interpolate(
        b1.points.len(), &b1.verbs, &b1.conics, b2.points.len(), &b2.verbs, &b2.conics
    ));
    let mid = interpolate_points(&b1.points, &b2.points, 0.5);
    assert_eq!(mid[0], Point::new(0.0, 5.0));
    assert_eq!(mid[1], Point::new(15.0, 5.0));
}

#[test]
fn test_contour_and_path_measure() {
    let mut pb = PathBuilder::new(FillType::Winding);
    pb.add_rect(Rect::from_ltrb(0.0, 0.0, 100.0, 50.0), Direction::Cw, 0)
        .move_to(Point::new(0.0, 0.0))
        .quad_to(Point::new(50.0, 100.0), Point::new(100.0, 0.0));
    let buf = pb.into_buf();
    let raw = buf.as_raw(false).unwrap();

    let mut meas = PathMeasure::new(&raw, false, 1.0);
    assert!(meas.is_closed());
    assert_near(meas.get_length(), 300.0, 1e-3);

    let mut pos = Point::ZERO;
    let mut tan = Vector::ZERO;
    assert!(meas.get_pos_tan(50.0, Some(&mut pos), Some(&mut tan)));
    assert_near(pos.x, 50.0, 1e-3);
    assert_near(pos.y, 0.0, 1e-3);
    assert_near(tan.x, 1.0, 1e-3);
    assert_near(tan.y, 0.0, 1e-3);

    let m = meas.get_matrix(125.0, GET_POS_AND_TAN_MATRIX_FLAG).unwrap();
    assert_near(m.mat[2], 100.0, 1e-3);
    assert_near(m.mat[5], 25.0, 1e-3);

    let _ = meas.get_matrix(10.0, GET_POSITION_MATRIX_FLAG).unwrap();
    let _ = meas.get_matrix(10.0, GET_TANGENT_MATRIX_FLAG).unwrap();

    let mut seg_dst = PathBuilder::new(FillType::Winding);
    assert!(meas.get_segment(50.0, 150.0, &mut seg_dst, true));
    let seg_buf = seg_dst.into_buf();
    assert_eq!(seg_buf.points[0], Point::new(50.0, 0.0));
    assert_eq!(*seg_buf.points.last().unwrap(), Point::new(100.0, 50.0));

    // Second contour (quadratic curve)
    assert!(meas.next_contour());
    assert!(!meas.is_closed());
    assert!(meas.get_length() > 100.0);
    assert!(!meas.next_contour());

    // Conic circle circumference check
    let mut cpb = PathBuilder::new(FillType::Winding);
    cpb.add_circle(Point::new(0.0, 0.0), 100.0, Direction::Cw);
    let cbuf = cpb.into_buf();
    let craw = cbuf.as_raw(false).unwrap();
    let mut citer = ContourMeasureIter::new(&craw, false, 2.0);
    let cm = citer.next_contour().unwrap();
    assert!(cm.is_closed());
    let expected_circ = 2.0 * core::f32::consts::PI * 100.0;
    assert_near(cm.length(), expected_circ, 0.5);
}

#[test]
fn test_stroke_rec_and_stroker() {
    let mut rec = StrokeRec::new_fill();
    assert_eq!(rec.get_style(), StrokeStyle::Fill);
    assert_eq!(rec.get_inflation_radius(), 0.0);

    rec.set_hairline_style();
    assert_eq!(rec.get_style(), StrokeStyle::Hairline);
    assert_eq!(rec.get_inflation_radius(), 1.0);

    rec.set_stroke_style(10.0, false);
    rec.set_stroke_params(Cap::Square, Join::Miter, 4.0);
    assert_eq!(rec.get_style(), StrokeStyle::Stroke);
    assert_near(rec.get_inflation_radius(), 20.0, 1e-4);

    // Stroke a rect
    let mut rpb = PathBuilder::new(FillType::Winding);
    rpb.add_rect(Rect::from_ltrb(10.0, 10.0, 110.0, 110.0), Direction::Cw, 0);
    let rbuf = rpb.into_buf();
    let rraw = rbuf.as_raw(true).unwrap();

    let mut dst = PathBuilder::new(FillType::Winding);
    assert!(rec.apply_to_path(&mut dst, &rraw));
    let stroked = dst.into_buf();
    let bounds = trimmed_bounds(&stroked.points, &stroked.verbs).unwrap();
    assert_eq!(bounds, Rect::from_ltrb(5.0, 5.0, 115.0, 115.0));

    // Stroke a cubic curve with round cap and round join
    let mut cpb = PathBuilder::new(FillType::Winding);
    cpb.move_to(Point::new(0.0, 0.0))
        .cubic_to(
            Point::new(30.0, 80.0),
            Point::new(70.0, -80.0),
            Point::new(100.0, 0.0),
        )
        .quad_to(Point::new(150.0, 50.0), Point::new(200.0, 0.0));
    let cbuf = cpb.into_buf();
    let craw = cbuf.as_raw(false).unwrap();

    let stroker = Stroke {
        width: 6.0,
        miter_limit: 4.0,
        res_scale: 1.0,
        cap: Cap::Round,
        join: Join::Round,
        do_fill: false,
    };
    let mut cdst = PathBuilder::new(FillType::Winding);
    stroker.stroke_path(&craw, &mut cdst);
    let cstroked = cdst.into_buf();
    assert!(!cstroked.is_empty());
    assert!(cstroked.is_finite());

    // Zero-length line with round cap should produce a closed circle-like shape
    let mut zpb = PathBuilder::new(FillType::Winding);
    zpb.move_to(Point::new(50.0, 50.0))
        .line_to(Point::new(50.0, 50.0));
    let zbuf = zpb.into_buf();
    let zraw = zbuf.as_raw(false).unwrap();
    let mut zdst = PathBuilder::new(FillType::Winding);
    stroker.stroke_path(&zraw, &mut zdst);
    assert!(!zdst.is_empty());
}

#[test]
fn test_serialization_and_dump() {
    let mut pb = PathBuilder::new(FillType::EvenOdd);
    pb.move_to(Point::new(1.5, 2.0))
        .line_to(Point::new(10.0, 20.0))
        .conic_to(Point::new(30.0, 40.0), Point::new(50.0, 60.0), 0.70710677)
        .close();
    let buf = pb.into_buf();

    let needed = write_to_memory(buf.fill_type, &buf.points, &buf.verbs, &buf.conics, None, None);
    assert!(needed > 16);
    assert_eq!(needed % 4, 0);

    let mut storage = vec![0u8; needed];
    let written = write_to_memory(
        buf.fill_type,
        &buf.points,
        &buf.verbs,
        &buf.conics,
        None,
        Some(&mut storage),
    );
    assert_eq!(written, needed);

    match read_from_memory(&storage).unwrap() {
        ReadPathResult::General {
            buf: decoded,
            bytes_read,
        } => {
            assert_eq!(bytes_read, needed);
            assert_eq!(decoded.fill_type, FillType::EvenOdd);
            assert_eq!(decoded.points, buf.points);
            assert_eq!(decoded.verbs, buf.verbs);
            assert_eq!(decoded.conics, buf.conics);
        }
        _ => panic!("expected general path"),
    }

    // RRect serialization roundtrip
    let mut rr = RRect::make_empty();
    rr.set_rect_xy(Rect::from_ltrb(0.0, 0.0, 40.0, 30.0), 5.0, 5.0);
    let info = RRectSerialInfo {
        rrect: rr,
        direction: Direction::Ccw,
        start_index: 2,
    };
    let rr_size = write_to_memory(FillType::InverseWinding, &[], &[], &[], Some(&info), None);
    let mut rr_buf = vec![0u8; rr_size];
    assert_eq!(
        write_to_memory(
            FillType::InverseWinding,
            &[],
            &[],
            &[],
            Some(&info),
            Some(&mut rr_buf)
        ),
        rr_size
    );
    match read_from_memory(&rr_buf).unwrap() {
        ReadPathResult::RRect {
            rrect,
            direction,
            start_index,
            fill_type,
            bytes_read,
        } => {
            assert_eq!(bytes_read, rr_size);
            assert_eq!(rrect.rect, rr.rect);
            assert_eq!(direction, Direction::Ccw);
            assert_eq!(start_index, 2);
            assert_eq!(fill_type, FillType::InverseWinding);
        }
        _ => panic!("expected rrect path"),
    }

    // Dump checks
    let dec = dump_path_to_string(buf.fill_type, &buf.points, &buf.verbs, &buf.conics, false);
    assert!(dec.contains("path.setFillType(SkPathFillType::kEvenOdd);"));
    assert!(dec.contains("path.moveTo(1.5f, 2);"));
    assert!(dec.contains("path.close();"));

    let hex = dump_builder_to_string(buf.fill_type, &buf.points, &buf.verbs, &buf.conics, true);
    assert!(hex.contains("SkPathBuilder(SkPathFillType::kEvenOdd)"));
    assert!(hex.contains("SkBits2Float(0x"));
}

#[test]
fn test_dash_path() {
    assert!(valid_dash_path(0.0, &[10.0, 5.0]));
    assert!(!valid_dash_path(0.0, &[10.0]));
    assert!(!valid_dash_path(0.0, &[0.0, 0.0]));
    assert!(!valid_dash_path(f32::NAN, &[10.0, 5.0]));

    let p = calc_dash_parameters(-5.0, &[10.0, 5.0], true);
    assert_near(p.interval_length, 15.0, 1e-4);
    assert_near(p.adjusted_phase, 10.0, 1e-4);
    assert_eq!(p.initial_dash_index, 1);
    assert_near(p.initial_dash_length, 5.0, 1e-4);

    // Dash a horizontal line with special-line optimization
    let mut pb = PathBuilder::new(FillType::Winding);
    pb.move_to(Point::new(0.0, 0.0)).line_to(Point::new(100.0, 0.0));
    let buf = pb.into_buf();
    let raw = buf.as_raw(false).unwrap();

    let mut rec = StrokeRec::new_hairline();
    rec.set_stroke_style(4.0, false);
    rec.set_stroke_params(Cap::Butt, Join::Miter, 4.0);

    let mut dst = PathBuilder::new(FillType::Winding);
    assert!(filter_dash_path(&mut dst, &raw, &mut rec, None, 0.0, &[10.0, 10.0]));
    // SpecialLineRec converts butt-capped stroked line directly to filled quads!
    assert_eq!(rec.get_style(), StrokeStyle::Fill);
    assert!(!dst.is_empty());

    // Dash a closed rect with cull rect
    let mut rpb = PathBuilder::new(FillType::Winding);
    rpb.add_rect(Rect::from_ltrb(0.0, 0.0, 100.0, 100.0), Direction::Cw, 0);
    let rbuf = rpb.into_buf();
    let rraw = rbuf.as_raw(false).unwrap();
    let mut rec2 = StrokeRec::new_hairline();
    let mut dst2 = PathBuilder::new(FillType::Winding);
    let cull = Rect::from_ltrb(-10.0, -10.0, 50.0, 50.0);
    assert!(filter_dash_path(
        &mut dst2,
        &rraw,
        &mut rec2,
        Some(cull),
        0.0,
        &[10.0, 5.0]
    ));
    assert!(!dst2.is_empty());
}

#[test]
fn test_parse_svg_path_roundtrip() {
    let svg = "M 0 0 L 10 20 H 30 V 40 Q 50 50 60 40 T 80 40 C 90 10 100 10 110 40 S 130 70 140 40 Z";
    let buf = from_svg_string(svg.as_bytes()).unwrap();
    assert!(!buf.is_empty());
    assert_eq!(buf.verbs[0], Verb::Move);
    assert_eq!(*buf.verbs.last().unwrap(), Verb::Close);

    // Compact SVG notation without spaces
    let compact = from_svg_string(b"M10-20l5.5.5-2-3z").unwrap();
    assert_eq!(compact.points[0], Point::new(10.0, -20.0));
    assert_near(compact.points[1].x, 15.5, 1e-4);
    assert_near(compact.points[1].y, -19.5, 1e-4);

    // Arc command
    let arc = from_svg_string(b"M 10 80 A 45 45 0 0 0 125 125").unwrap();
    assert!(arc.verbs.contains(&Verb::Conic));

    // Invalid SVG strings
    assert!(from_svg_string(b"10 20").is_none());
    assert!(from_svg_string(b"M 10").is_none());
    assert!(from_svg_string(b"M 0 0 X 10 10").is_none());

    // Roundtrip to SVG string (Absolute & Relative)
    let abs_str = to_svg_string(&buf.points, &buf.verbs, &buf.conics, PathEncoding::Absolute);
    assert!(abs_str.starts_with("M0 0L10 20"));
    assert!(abs_str.ends_with('Z'));

    // LegacyPathIter emits the implicit closing Line back to (0, 0) before Z, adding 1 point.
    let reparsed = from_svg_string(abs_str.as_bytes()).unwrap();
    assert_eq!(reparsed.points.len(), buf.points.len() + 1);
    assert_eq!(*reparsed.points.last().unwrap(), Point::new(0.0, 0.0));

    let rel_str = to_svg_string(&buf.points, &buf.verbs, &buf.conics, PathEncoding::Relative);
    assert!(rel_str.starts_with("m0 0l10 20"));
}
