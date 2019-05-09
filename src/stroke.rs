use crate::path_builder::{Path, PathOp, PathBuilder};
use euclid::{Point2D, Vector2D};

type Point = Point2D<f32>;
type Vector = Vector2D<f32>;

pub struct StrokeStyle {
    pub width: f32,
    pub cap: LineCap,
    pub join: LineJoin,
    pub mitre_limit: f32,
    pub dash_array: Vec<f32>,
    pub dash_offset: f32,
}

pub enum LineCap {
    Round,
    Square,
    Butt
}

pub enum LineJoin {
    Round,
    Mitre,
    Bevel,
}

fn compute_normal(p0: Point, p1: Point) -> Vector {
    let ux = p1.x - p0.x;
    let uy = p1.y - p0.y;

    // this could overflow f32. Skia checks for this and
    // uses a double in that situation
    let ulen = ux.hypot(uy);
    assert!(ulen != 0.);
    // the normal is perpendicular to the *unit* vector
    Vector::new(-uy/ulen, ux/ulen)
}

fn flip(v: Vector) -> Vector {
    Vector::new(-v.x, -v.y)
}

/* Compute a spline approximation of the arc
   centered at xc, yc from the angle a to the angle b

   The angle between a and b should not be more than a
   quarter circle (pi/2)

   The approximation is similar to an approximation given in:
   "Approximation of a cubic bezier curve by circular arcs and vice versa"
   by Alekas Riškus. However that approximation becomes unstable when the
   angle of the arc approaches 0.

   This approximation is inspired by a discusion with Boris Zbarsky
   and essentially just computes:

     h = 4.0/3.0 * tan ((angle_B - angle_A) / 4.0);

   without converting to polar coordinates.

   A different way to do this is covered in "Approximation of a cubic bezier
   curve by circular arcs and vice versa" by Alekas Riškus. However, the method
   presented there doesn't handle arcs with angles close to 0 because it
   divides by the perp dot product of the two angle vectors.
   */
fn arc_segment(path: &mut PathBuilder,
               xc: f32,
               yc: f32,
               radius: f32,
               a: Vector,
               b: Vector)
{
    let r_sin_A = radius * a.y;
    let r_cos_A = radius * a.x;
    let r_sin_B = radius * b.y;
    let r_cos_B = radius * b.x;

    /* bisect the angle between 'a' and 'b' with 'mid' */
    let mut mid = a + b;
    mid /= mid.length();

    /* bisect the angle between 'a' and 'mid' with 'mid2' this is parallel to a
     * line with angle (B - A)/4 */
    let mid2 = a + mid;

    let h = (4. / 3.) * dot(perp(a), mid2) / dot(a, mid2);

    path.cubic_to(
        xc + r_cos_A - h * r_sin_A,
        yc + r_sin_A + h * r_cos_A,
        xc + r_cos_B + h * r_sin_B,
        yc + r_sin_B - h * r_cos_B,
        xc + r_cos_B,
        yc + r_sin_B);
}

/* The angle between the vectors must be <= pi */
fn bisect(a: Vector, b: Vector) -> Vector
{
    let mut mid;
    if dot(a, b) >= 0. {
        /* if the angle between a and b is accute, then we can
         * just add the vectors and normalize */
        mid = a + b;
    } else {
        /* otherwise, we can flip a, add it
         * and then use the perpendicular of the result */
        mid = flip(a) + b;
        mid = perp(mid);
    }

    /* normalize */
    /* because we assume that 'a' and 'b' are normalized, we can use
     * sqrt instead of hypot because the range of mid is limited */
    let mid_len = mid.x * mid.x + mid.y * mid.y;
    let len = mid_len.sqrt();
    return mid / len;
}

fn arc(path: &mut PathBuilder, xc: f32, yc: f32, radius: f32, a: Vector, b: Vector)
{
    /* find a vector that bisects the angle between a and b */
    let mid_v = bisect(a, b);

    /* construct the arc using two curve segments */
    arc_segment(path, xc, yc, radius, a, mid_v);
    arc_segment(path, xc, yc, radius, mid_v, b);
}

fn join_round(path: &mut PathBuilder, center: Point, a: Vector, b: Vector, radius: f32)
{
    /*
    int ccw = dot (perp (b), a) >= 0; // XXX: is this always true?
    yes, otherwise we have an interior angle.
    assert (ccw);
    */
    arc(path, center.x, center.y, radius, a, b);
}

fn cap_line(dest: &mut PathBuilder, style: &StrokeStyle, pt: Point, normal: Vector) {
    let offset = style.width / 2.;
    match style.cap {
        LineCap::Butt => { /* nothing to do */ },
        LineCap::Round => {
            dest.move_to(pt.x + normal.x * offset, pt.y + normal.y * offset);
            arc (dest, pt.x, pt.y, offset, normal, flip(normal));
            dest.close();
        },
        LineCap::Square => {
            // parallel vector
            let v = Vector::new(normal.y, -normal.x);
            let end = pt + v * offset;
            dest.move_to(pt.x + normal.x * offset, pt.y + normal.y * offset);
            dest.line_to(end.x + normal.x * offset, end.y + normal.y * offset);
            dest.line_to(end.x + -normal.x * offset, end.y + -normal.y * offset);
            dest.line_to(pt.x - normal.x * offset, pt.y - normal.y * offset);
            dest.close();
        },
    }
}

fn bevel(dest: &mut PathBuilder, style: &StrokeStyle, pt: Point, s1_normal: Vector, s2_normal: Vector) {
    let offset = style.width / 2.;
    dest.move_to(pt.x + s1_normal.x * offset, pt.y + s1_normal.y * offset);
    dest.line_to(pt.x + s2_normal.x * offset, pt.y + s2_normal.y * offset);
    dest.line_to(pt.x, pt.y);
    dest.close();
}

/* given a normal rotate the vector 90 degrees to the right clockwise
 * This function has a period of 4. e.g. swap(swap(swap(swap(x) == x */
fn swap(a: Vector) -> Vector
{
    /* one of these needs to be negative. We choose a.x so that we rotate to the right instead of negating */
    return Vector::new(a.y, -a.x);
}

fn unperp(a: Vector) -> Vector
{
    swap(a)
}

/* rotate a vector 90 degrees to the left */
fn perp(v: Vector) -> Vector
{
    Vector::new(-v.y, v.x)
}

fn dot(a: Vector, b: Vector) -> f32
{
    a.x * b.x + a.y * b.y
}

/* Finds the intersection of two lines each defined by a point and a normal.
   From "Example 2: Find the intersection of two lines" of
   "The Pleasures of "Perp Dot" Products"
   F. S. Hill, Jr. */
fn line_intersection(A: Point, a_perp: Vector, B: Point, b_perp: Vector) -> Point
{
    let a = unperp(a_perp);
    let c = B - A;
    let denom = dot(b_perp, a);
    if denom == 0.0 {
        panic!("trouble")
    }

    let t = dot(b_perp, c) / denom;

    let intersection = Point::new(A.x + t * (a.x),
                                  A.y + t * (a.y));

    intersection
}

fn is_interior_angle(a: Vector, b: Vector) -> bool {
    /* angles of 180 and 0 degress will evaluate to 0, however
     * we to treat 180 as an interior angle and 180 as an exterior angle */
    dot(perp(a), b) > 0. || a == b /* 0 degrees is interior */
}

fn join_line(dest: &mut PathBuilder, style: &StrokeStyle, pt: Point, mut s1_normal: Vector, mut s2_normal: Vector) {

    if is_interior_angle(s1_normal, s2_normal) {
        s2_normal = flip(s2_normal);
        s1_normal = flip(s1_normal);
        std::mem::swap(&mut s1_normal, &mut s2_normal);
    }

    // XXX: joining uses `pt` which can cause seams because it lies halfway on a line and the
    // rasterizer may not find exactly the same spot
    let offset = style.width / 2.;
    match style.join {
        LineJoin::Round => {
            dest.move_to(pt.x + s1_normal.x * offset, pt.y + s1_normal.y * offset);
            arc (dest, pt.x, pt.y, offset, s1_normal, s2_normal);
            dest.line_to(pt.x, pt.y);
            dest.close();
        },
        LineJoin::Mitre => {
            let in_dot_out = -s1_normal.x * s2_normal.x + -s1_normal.y * s2_normal.y;
            if 2. <= style.mitre_limit*style.mitre_limit * (1. - in_dot_out) {
                let start = pt + s1_normal * offset;
                let end = pt + s2_normal * offset;
                let intersection = line_intersection(start, s1_normal, end, s2_normal);
                dest.move_to(pt.x + s1_normal.x * offset, pt.y + s1_normal.y * offset);
                dest.line_to(intersection.x, intersection.y);
                dest.line_to(pt.x + s2_normal.x * offset, pt.y + s2_normal.y * offset);
                dest.line_to(pt.x, pt.y);
                dest.close();
            } else {
                bevel(dest, style, pt, s1_normal, s2_normal);
            }
        },
        LineJoin::Bevel => {
            bevel(dest, style, pt, s1_normal, s2_normal);
        },
    }
}


pub fn stroke_to_path(path: &Path, style: &StrokeStyle) -> Path {
    let mut cur_x = 0.;
    let mut cur_y = 0.;
    let mut stroked_path = PathBuilder::new();
    let mut last_normal = Vector::zero();
    let half_width = style.width / 2.;
    let mut start_point = None;
    for op in &path.ops {
        match *op {
            PathOp::MoveTo(x, y) => {
                if let Some((point, normal)) = start_point {
                    // cap end
                    cap_line(&mut stroked_path, style, Point::new(cur_x, cur_y), last_normal);
                    // cap beginning
                    cap_line(&mut stroked_path, style, point, flip(normal));
                }
                start_point = None;
                cur_x = x;
                cur_y = y;
            }
            PathOp::LineTo(x, y) => {
                let normal = compute_normal(Point2D::new(cur_x, cur_y), Point2D::new(x, y));
                if start_point.is_none() {
                    start_point = Some((Point::new(cur_x, cur_y), normal));
                } else {
                    join_line(&mut stroked_path, style, Point::new(cur_x, cur_y), last_normal, normal);
                }

                stroked_path.move_to(cur_x + normal.x * half_width, cur_y + normal.y * half_width);
                stroked_path.line_to(x + normal.x * half_width, y + normal.y * half_width);
                stroked_path.line_to(x + -normal.x * half_width, y + -normal.y * half_width);
                stroked_path.line_to(cur_x - normal.x * half_width, cur_y - normal.y * half_width);
                stroked_path.close();
                last_normal = normal;

                cur_x = x;
                cur_y = y;

            }
            PathOp::Close => {
                if let Some((point, normal)) = start_point {
                    let last_normal = compute_normal(Point2D::new(cur_x, cur_y), Point2D::new(point.x, point.y));

                    stroked_path.move_to(cur_x + normal.x * half_width, cur_y + normal.y * half_width);
                    stroked_path.line_to(point.x + normal.x * half_width, point.y + normal.y * half_width);
                    stroked_path.line_to(point.x + -normal.x * half_width, point.y + -normal.y * half_width);
                    stroked_path.line_to(cur_x - normal.x * half_width, cur_y - normal.y * half_width);
                    stroked_path.close();

                    join_line(&mut stroked_path, style, point, last_normal, normal);
                }
            },
            PathOp::QuadTo(..) => {
                panic!("Only flat paths handled")
            }
            PathOp::CubicTo(..) => {
                panic!("Only flat paths handled")
            }
        }
    }
    if let Some((point, normal)) = start_point {
        // cap end
        cap_line(&mut stroked_path, style, Point::new(cur_x, cur_y), last_normal);
        // cap beginning
        cap_line(&mut stroked_path, style, point, flip(normal));
    }
    stroked_path.finish()
}