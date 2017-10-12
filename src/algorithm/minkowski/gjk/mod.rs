//! GJK distance/collision detection algorithm. For now only have implementation of collision
//! detection, not distance computation.

pub use self::simplex::SimplexProcessor;

use std::cmp::Ordering;
use std::ops::{Neg, Range};

use cgmath::BaseFloat;
use cgmath::prelude::*;
use num::{Float, NumCast};

use self::simplex::{SimplexProcessor2, SimplexProcessor3};
use {CollisionStrategy, Contact};
use algorithm::minkowski::{EPA2, EPA3, SupportPoint, EPA};
use prelude::*;

mod simplex;

const MAX_ITERATIONS: u32 = 100;
const GJK_DISTANCE_TOLERANCE: f32 = 0.000001;
const GJK_CONTINUOUS_TOLERANCE: f32 = 0.000001;

/// GJK algorithm for 2D, see [GJK](struct.GJK.html) for more information.
pub type GJK2<S> = GJK<SimplexProcessor2<S>, EPA2<S>>;

/// GJK algorithm for 3D, see [GJK](struct.GJK.html) for more information.
pub type GJK3<S> = GJK<SimplexProcessor3<S>, EPA3<S>>;

/// Gilbert-Johnson-Keerthi narrow phase collision detection algorithm.
///
/// # Type parameters:
///
/// - `S`: simplex processor type. Should be either
///        [`SimplexProcessor2`](struct.SimplexProcessor2.html) or
///        [`SimplexProcessor3`](struct.SimplexProcessor3.html)
/// - `E`: EPA algorithm implementation type. Should be either
///        [`EPA2`](struct.EPA2.html) or
///        [`EPA3`](struct.EPA3.html)
///
#[derive(Debug)]
pub struct GJK<SP, E> {
    simplex_processor: SP,
    epa: E,
}

impl<SP, E> GJK<SP, E>
where
    SP: SimplexProcessor,
    <SP::Point as EuclideanSpace>::Scalar: BaseFloat,
    E: EPA<Point = SP::Point>,
{
    /// Create a new GJK algorithm implementation
    pub fn new() -> Self {
        Self {
            simplex_processor: SP::new(),
            epa: E::new(),
        }
    }

    /// Do intersection test on the given primitives
    ///
    /// ## Parameters:
    ///
    /// - `left`: left primitive
    /// - `left_transform`: model-to-world-transform for the left primitive
    /// - `right`: right primitive,
    /// - `right_transform`: model-to-world-transform for the right primitive
    ///
    /// ## Returns:
    ///
    /// Will return a simplex if a collision was detected. For 2D, the simplex will be a triangle,
    /// for 3D, it will be a tetrahedron. The simplex will enclose the origin.
    /// If no collision was detected, None is returned.
    ///
    pub fn intersect<P, PL, PR, TL, TR>(
        &self,
        left: &PL,
        left_transform: &TL,
        right: &PR,
        right_transform: &TR,
    ) -> Option<Vec<SupportPoint<P>>>
    where
        P: EuclideanSpace,
        P::Scalar: BaseFloat,
        PL: SupportFunction<Point = P>,
        PR: SupportFunction<Point = P>,
        SP: SimplexProcessor<Point = P>,
        P::Diff: Neg<Output = P::Diff> + InnerSpace,
        TL: Transform<P>,
        TR: Transform<P>,
    {
        let right_pos = right_transform.transform_point(P::origin());
        let left_pos = left_transform.transform_point(P::origin());
        let mut d = right_pos - left_pos;
        let a = SupportPoint::from_minkowski(left, left_transform, right, right_transform, &d);
        if a.v.dot(d) <= P::Scalar::zero() {
            return None;
        }
        let mut simplex: Vec<SupportPoint<P>> = Vec::default();
        simplex.push(a);
        d = d.neg();
        for _ in 0..MAX_ITERATIONS {
            let a = SupportPoint::from_minkowski(left, left_transform, right, right_transform, &d);
            if a.v.dot(d) <= P::Scalar::zero() {
                return None;
            } else {
                simplex.push(a);
                if self.simplex_processor
                    .reduce_to_closest_feature(&mut simplex, &mut d)
                {
                    return Some(simplex);
                }
            }
        }

        None
    }

    /// Do time of impact intersection testing on the given primitives, and return a valid collision
    /// time of impact.
    ///
    /// ## Parameters:
    ///
    /// - `left`: left primitive
    /// - `left_transform`: model-to-world-transform for the left primitive
    /// - `right`: right primitive,
    /// - `right_transform`: model-to-world-transform for the right primitive
    ///
    /// ## Returns:
    ///
    /// Will optionally return the time of impact. If no collision was detected, None is returned.
    #[allow(unused_variables)]
    pub fn intersect_time_of_impact<P, PL, PR, TL, TR>(
        &self,
        left: &PL,
        left_transform: Range<&TL>,
        right: &PR,
        right_transform: Range<&TR>,
    ) -> Option<P::Scalar>
    where
        P: EuclideanSpace,
        P::Scalar: BaseFloat,
        PL: SupportFunction<Point = P>,
        PR: SupportFunction<Point = P>,
        SP: SimplexProcessor<Point = P>,
        P::Diff: Neg<Output = P::Diff> + InnerSpace,
        TL: Transform<P> + TranslationInterpolate<P::Scalar>,
        TR: Transform<P> + TranslationInterpolate<P::Scalar>,
    {
        let tolerance: P::Scalar = NumCast::from(GJK_CONTINUOUS_TOLERANCE).unwrap();
        // build the ray, A.velocity - B.velocity is the ray direction
        let left_lin_vel = left_transform.end.transform_point(P::origin())
            - left_transform.start.transform_point(P::origin());
        let right_lin_vel = right_transform.end.transform_point(P::origin())
            - right_transform.start.transform_point(P::origin());
        let r = left_lin_vel - right_lin_vel;

        // initialize time of impact
        let mut lambda = P::Scalar::zero();

        // build the start transforms
        let mut left_curr_transform = left_transform
            .start
            .translation_interpolate(left_transform.end, lambda);
        let mut right_curr_transform = right_transform
            .start
            .translation_interpolate(right_transform.end, lambda);

        // build simplex and get the first support point
        let mut simplex = Vec::with_capacity(5);
        simplex.push(SupportPoint::from_minkowski(
            left,
            &left_curr_transform,
            right,
            &right_curr_transform,
            &-r,
        ));
        let mut d = simplex[0].v.clone();
        for _ in 0..MAX_ITERATIONS {
            // d almost at origin means we have a hit
            if d.magnitude2() <= tolerance {
                return Some(lambda);
            }

            // time of impact > 1 means miss
            if lambda > P::Scalar::one() {
                return None;
            }

            let p = SupportPoint::from_minkowski(
                left,
                &left_curr_transform,
                right,
                &right_curr_transform,
                &-d,
            );

            let vp = d.dot(p.v);
            if vp > P::Scalar::zero() {
                let vr = d.dot(r);
                if vr >= -tolerance {
                    return None;
                } else {
                    // we have a potential hit, move origin forwards along the ray
                    lambda = lambda - vp / vr;

                    // interpolate translation of shapes along the ray
                    left_curr_transform = left_transform
                        .start
                        .translation_interpolate(left_transform.end, lambda);
                    right_curr_transform = right_transform
                        .start
                        .translation_interpolate(right_transform.end, lambda);
                }
            }
            simplex.push(p);

            // if reduction returns true, we have a hit, so return time of impact
            // if not, the simplex is reduced to the closest feature to the origin, and v is the
            // normal of the feature in the direction of the origin
            if self.simplex_processor
                .reduce_to_closest_feature(&mut simplex, &mut d)
            {
                return Some(lambda);
            }
        }
        None
    }

    /// Compute the distance between the given primitives.
    ///
    /// ## Parameters:
    ///
    /// - `left`: left primitive
    /// - `left_transform`: model-to-world-transform for the left primitive
    /// - `right`: right primitive,
    /// - `right_transform`: model-to-world-transform for the right primitive
    ///
    /// ## Returns:
    ///
    /// Will optionally return the distance between the objects. Will return None, if the objects
    /// are colliding.
    pub fn distance<P, PL, PR, TL, TR>(
        &self,
        left: &PL,
        left_transform: &TL,
        right: &PR,
        right_transform: &TR,
    ) -> Option<P::Scalar>
    where
        P: EuclideanSpace,
        P::Scalar: BaseFloat,
        PL: SupportFunction<Point = P>,
        PR: SupportFunction<Point = P>,
        SP: SimplexProcessor<Point = P>,
        P::Diff: Neg<Output = P::Diff> + InnerSpace,
        TL: Transform<P>,
        TR: Transform<P>,
    {
        let zero = P::Diff::zero();
        let tolerance: P::Scalar = NumCast::from(GJK_DISTANCE_TOLERANCE).unwrap();
        let right_pos = right_transform.transform_point(P::origin());
        let left_pos = left_transform.transform_point(P::origin());
        let mut simplex = Vec::with_capacity(5);
        for d in &[right_pos - left_pos, left_pos - right_pos] {
            simplex.push(SupportPoint::from_minkowski(
                left,
                left_transform,
                right,
                right_transform,
                &d,
            ));
        }
        for _ in 0..MAX_ITERATIONS {
            let d = self.simplex_processor
                .get_closest_point_to_origin(&mut simplex);
            if ulps_eq!(d, zero) {
                return None;
            }
            let d = d.neg();
            let p = SupportPoint::from_minkowski(left, left_transform, right, right_transform, &d);
            let dp = p.v.dot(d);
            let d0 = simplex[0].v.dot(d);
            if dp - d0 < tolerance {
                return Some(d.magnitude());
            }
            simplex.push(p);
        }
        None
    }

    /// Given a GJK simplex that encloses the origin, compute the contact manifold.
    ///
    /// Uses the EPA algorithm to find the contact information from the simplex.
    pub fn get_contact_manifold<P, PL, PR, TL, TR>(
        &self,
        mut simplex: &mut Vec<SupportPoint<P>>,
        left: &PL,
        left_transform: &TL,
        right: &PR,
        right_transform: &TR,
    ) -> Option<Contact<P>>
    where
        P: EuclideanSpace,
        PL: SupportFunction<Point = P>,
        PR: SupportFunction<Point = P>,
        TL: Transform<P>,
        TR: Transform<P>,
        SP: SimplexProcessor<Point = P>,
    {
        self.epa
            .process(&mut simplex, left, left_transform, right, right_transform)
    }

    /// Do intersection testing on the given primitives, and return the contact manifold.
    ///
    /// ## Parameters:
    ///
    /// - `strategy`: strategy to use, if `CollisionOnly` it will only return a boolean result,
    ///               otherwise, EPA will be used to compute the exact contact point.
    /// - `left`: left primitive
    /// - `left_transform`: model-to-world-transform for the left primitive
    /// - `right`: right primitive,
    /// - `right_transform`: model-to-world-transform for the right primitive
    ///
    /// ## Returns:
    ///
    /// Will optionally return a `Contact` if a collision was detected. In `CollisionOnly` mode,
    /// this contact will only be a boolean result. For `FullResolution` mode, the contact will
    /// contain a full manifold (collision normal, penetration depth and contact point).
    pub fn intersection<P, PL, PR, TL, TR>(
        &self,
        strategy: &CollisionStrategy,
        left: &PL,
        left_transform: &TL,
        right: &PR,
        right_transform: &TR,
    ) -> Option<Contact<P>>
    where
        P: EuclideanSpace,
        P::Scalar: BaseFloat,
        P::Diff: Neg<Output = P::Diff> + InnerSpace,
        PL: SupportFunction<Point = P>,
        PR: SupportFunction<Point = P>,
        TL: Transform<P>,
        TR: Transform<P>,
        SP: SimplexProcessor<Point = P>,
    {
        use CollisionStrategy::*;
        self.intersect(left, left_transform, right, right_transform)
            .and_then(|mut simplex| match *strategy {
                CollisionOnly => Some(Contact::new(CollisionOnly)),
                FullResolution => {
                    self.get_contact_manifold(
                        &mut simplex,
                        left,
                        left_transform,
                        right,
                        right_transform,
                    )
                }
            })
    }

    /// Do intersection test on the given complex shapes, and return the actual intersection point
    ///
    /// ## Parameters:
    ///
    /// - `strategy`: strategy to use, if `CollisionOnly` it will only return a boolean result,
    ///               otherwise, EPA will be used to compute the exact contact point.
    /// - `left`: shape consisting of a slice of primitive + local-to-model-transform for each
    ///           primitive,
    /// - `left_transform`: model-to-world-transform for the left shape
    /// - `right`: shape consisting of a slice of primitive + local-to-model-transform for each
    ///           primitive,
    /// - `right_transform`: model-to-world-transform for the right shape
    ///
    /// ## Returns:
    ///
    /// Will optionally return a `Contact` if a collision was detected. In `CollisionOnly` mode,
    /// this contact will only be a boolean result. For `FullResolution` mode, the contact will
    /// contain a full manifold (collision normal, penetration depth and contact point), for the
    /// contact with the highest penetration depth.
    pub fn intersection_complex<P, PL, PR, TL, TR>(
        &self,
        strategy: &CollisionStrategy,
        left: &[(PL, TL)],
        left_transform: &TL,
        right: &[(PR, TR)],
        right_transform: &TR,
    ) -> Option<Contact<P>>
    where
        P: EuclideanSpace,
        P::Scalar: BaseFloat,
        P::Diff: Neg<Output = P::Diff> + InnerSpace,
        PL: SupportFunction<Point = P>,
        PR: SupportFunction<Point = P>,
        TL: Transform<P>,
        TR: Transform<P>,
        SP: SimplexProcessor<Point = P>,
    {
        use CollisionStrategy::*;
        let mut contacts = Vec::default();
        for &(ref left_primitive, ref left_local_transform) in left.iter() {
            let left_transform = left_transform.concat(left_local_transform);
            for &(ref right_primitive, ref right_local_transform) in right.iter() {
                let right_transform = right_transform.concat(right_local_transform);
                if let Some(contact) = self.intersection(
                    strategy,
                    left_primitive,
                    &left_transform,
                    right_primitive,
                    &right_transform,
                ) {
                    match *strategy {
                        CollisionOnly => {
                            return Some(contact);
                        }
                        FullResolution => contacts.push(contact),
                    }
                }
            }
        }

        // CollisionOnly handling will have returned already if there was a contact, so this
        // scenario will only happen when we have a contact in FullResolution mode, or no contact
        // at all.
        contacts.into_iter().max_by(|l, r| {
            // Penetration depth defaults to 0., and can't be nan from EPA,
            // so unwrapping is safe
            l.penetration_depth
                .partial_cmp(&r.penetration_depth)
                .unwrap()
        })
    }

    /// Compute the distance between the given shapes.
    ///
    /// ## Parameters:
    ///
    /// - `left`: left shape
    /// - `left_transform`: model-to-world-transform for the left shape
    /// - `right`: right shape,
    /// - `right_transform`: model-to-world-transform for the right shape
    ///
    /// ## Returns:
    ///
    /// Will optionally return the smallest distance between the objects. Will return None, if the
    /// objects are colliding.
    pub fn distance_complex<P, PL, PR, TL, TR>(
        &self,
        left: &[(PL, TL)],
        left_transform: &TL,
        right: &[(PR, TR)],
        right_transform: &TR,
    ) -> Option<P::Scalar>
    where
        P: EuclideanSpace,
        P::Scalar: BaseFloat,
        P::Diff: Neg<Output = P::Diff> + InnerSpace,
        PL: SupportFunction<Point = P>,
        PR: SupportFunction<Point = P>,
        TL: Transform<P>,
        TR: Transform<P>,
        SP: SimplexProcessor<Point = P>,
    {
        let mut min_distance = None;
        for &(ref left_primitive, ref left_local_transform) in left.iter() {
            let left_transform = left_transform.concat(left_local_transform);
            for &(ref right_primitive, ref right_local_transform) in right.iter() {
                let right_transform = right_transform.concat(right_local_transform);
                match self.distance(
                    left_primitive,
                    &left_transform,
                    right_primitive,
                    &right_transform,
                ) {
                    None => return None, // colliding,
                    Some(distance) => {
                        min_distance = Some(
                            min_distance
                                .map_or(distance, |min_distance| distance.min(min_distance)),
                        )
                    }
                }
            }
        }

        min_distance
    }

    /// Do intersection time of impact test on the given complex shapes, and return the time of
    /// impact
    ///
    /// ## Parameters:
    ///
    /// - `strategy`: strategy to use, if `CollisionOnly` it will only return a boolean result,
    ///               otherwise, EPA will be used to compute the exact contact point.
    /// - `left`: shape consisting of a slice of primitive + local-to-model-transform for each
    ///           primitive,
    /// - `left_transform`: model-to-world-transform for the left shape
    /// - `right`: shape consisting of a slice of primitive + local-to-model-transform for each
    ///           primitive,
    /// - `right_transform`: model-to-world-transform for the right shape
    ///
    /// ## Returns:
    ///
    /// Will optionally return time of impact if a collision was detected.
    /// In `CollisionOnly` mode, this contact will only be a time of impact. For `FullResolution`
    /// mode, the time of impact will be the earliest found among all shape primitives.
    /// Will return None if no collision was found.
    pub fn intersect_complex_time_of_impact<P, PL, PR, TL, TR>(
        &self,
        strategy: &CollisionStrategy,
        left: &[(PL, TL)],
        left_transform: Range<&TL>,
        right: &[(PR, TR)],
        right_transform: Range<&TR>,
    ) -> Option<P::Scalar>
    where
        P: EuclideanSpace,
        P::Scalar: BaseFloat,
        P::Diff: Neg<Output = P::Diff> + InnerSpace,
        PL: SupportFunction<Point = P>,
        PR: SupportFunction<Point = P>,
        TL: Transform<P> + TranslationInterpolate<P::Scalar>,
        TR: Transform<P> + TranslationInterpolate<P::Scalar>,
        SP: SimplexProcessor<Point = P>,
    {
        use CollisionStrategy::*;
        let mut contacts = Vec::default();
        for &(ref left_primitive, ref left_local_transform) in left.iter() {
            let left_start_transform = left_transform.start.concat(left_local_transform);
            let left_end_transform = left_transform.end.concat(left_local_transform);
            for &(ref right_primitive, ref right_local_transform) in right.iter() {
                let right_start_transform = right_transform.start.concat(right_local_transform);
                let right_end_transform = right_transform.end.concat(right_local_transform);
                match self.intersect_time_of_impact(
                    left_primitive,
                    &left_start_transform..&left_end_transform,
                    right_primitive,
                    &right_start_transform..&right_end_transform,
                ) {
                    None => return None,
                    Some(toi) => {
                        match *strategy {
                            CollisionOnly => {
                                return Some(toi);
                            }
                            FullResolution => contacts.push(toi),
                        }
                    }
                };
            }
        }

        // CollisionOnly handling will have returned already if there was a contact, so this
        // scenario will only happen when we have a contact in FullResolution mode or no contact
        // at all
        contacts
            .into_iter()
            .min_by(|l, r| l.partial_cmp(&r).unwrap_or(Ordering::Equal))
    }
}

#[cfg(test)]
mod tests {
    use cgmath::{Basis2, Decomposed, Point2, Point3, Quaternion, Rad, Rotation2, Rotation3,
                 Vector2, Vector3};

    use super::*;
    use primitive::*;

    fn transform(x: f32, y: f32, angle: f32) -> Decomposed<Vector2<f32>, Basis2<f32>> {
        Decomposed {
            disp: Vector2::new(x, y),
            rot: Rotation2::from_angle(Rad(angle)),
            scale: 1.,
        }
    }

    fn transform_3d(
        x: f32,
        y: f32,
        z: f32,
        angle_z: f32,
    ) -> Decomposed<Vector3<f32>, Quaternion<f32>> {
        Decomposed {
            disp: Vector3::new(x, y, z),
            rot: Quaternion::from_angle_z(Rad(angle_z)),
            scale: 1.,
        }
    }

    #[test]
    fn test_gjk_miss() {
        let left = Rectangle::new(10., 10.);
        let left_transform = transform(15., 0., 0.);
        let right = Rectangle::new(10., 10.);
        let right_transform = transform(-15., 0., 0.);
        let gjk = GJK2::new();
        assert!(
            gjk.intersect(&left, &left_transform, &right, &right_transform)
                .is_none()
        );
        assert!(
            gjk.intersection(
                &CollisionStrategy::FullResolution,
                &left,
                &left_transform,
                &right,
                &right_transform
            ).is_none()
        )
    }

    #[test]
    fn test_gjk_hit() {
        let left = Rectangle::new(10., 10.);
        let left_transform = transform(15., 0., 0.);
        let right = Rectangle::new(10., 10.);
        let right_transform = transform(7., 2., 0.);
        let gjk = GJK2::new();
        let simplex = gjk.intersect(&left, &left_transform, &right, &right_transform);
        assert!(simplex.is_some());
        let contact = gjk.intersection(
            &CollisionStrategy::FullResolution,
            &left,
            &left_transform,
            &right,
            &right_transform,
        );
        assert!(contact.is_some());
        let contact = contact.unwrap();
        assert_eq!(Vector2::new(-1., 0.), contact.normal);
        assert_eq!(2., contact.penetration_depth);
        assert_eq!(Point2::new(10., 1.), contact.contact_point);
    }

    #[test]
    fn test_gjk_3d_hit() {
        let left = Cuboid::new(10., 10., 10.);
        let left_transform = transform_3d(15., 0., 0., 0.);
        let right = Cuboid::new(10., 10., 10.);
        let right_transform = transform_3d(7., 2., 0., 0.);
        let gjk = GJK3::new();
        let simplex = gjk.intersect(&left, &left_transform, &right, &right_transform);
        assert!(simplex.is_some());
        let contact = gjk.intersection(
            &CollisionStrategy::FullResolution,
            &left,
            &left_transform,
            &right,
            &right_transform,
        );
        assert!(contact.is_some());
        let contact = contact.unwrap();
        println!("{:?}", contact);
        assert_eq!(Vector3::new(-1., 0., 0.), contact.normal);
        assert_eq!(2., contact.penetration_depth);
        assert_ulps_eq!(Point3::new(10., 1., 5.), contact.contact_point);
    }

    #[test]
    fn test_gjk_distance_2d() {
        let left = Rectangle::new(10., 10.);
        let left_transform = transform(15., 0., 0.);
        let right = Rectangle::new(10., 10.);
        let right_transform = transform(0., 0., 0.);
        let gjk = GJK2::new();
        assert_eq!(
            Some(5.),
            gjk.distance(&left, &left_transform, &right, &right_transform)
        );

        // intersects
        let right_transform = transform(7., 2., 0.);
        assert_eq!(
            None,
            gjk.distance(&left, &left_transform, &right, &right_transform)
        );
    }

    #[test]
    fn test_gjk_distance_3d() {
        let left = Cuboid::new(10., 10., 10.);
        let left_transform = transform_3d(15., 0., 0., 0.);
        let right = Cuboid::new(10., 10., 10.);
        let right_transform = transform_3d(7., 2., 0., 0.);
        let gjk = GJK3::new();
        assert_eq!(
            None,
            gjk.distance(&left, &left_transform, &right, &right_transform)
        );

        let right_transform = transform_3d(1., 0., 0., 0.);
        assert_eq!(
            Some(4.),
            gjk.distance(&left, &left_transform, &right, &right_transform)
        );
    }

    #[test]
    fn test_gjk_time_of_impact_2d() {
        let left = Rectangle::new(10., 10.);
        let left_start_transform = transform(0., 0., 0.);
        let left_end_transform = transform(30., 0., 0.);
        let right = Rectangle::new(10., 10.);
        let right_transform = transform(15., 0., 0.);
        let gjk = GJK2::new();

        assert_ulps_eq!(
            0.1666667,
            gjk.intersect_time_of_impact(
                &left,
                &left_start_transform..&left_end_transform,
                &right,
                &right_transform..&right_transform
            ).unwrap()
        );

        assert_eq!(
            None,
            gjk.intersect_time_of_impact(
                &left,
                &left_start_transform..&left_start_transform,
                &right,
                &right_transform..&right_transform
            )
        );
    }

    #[test]
    fn test_gjk_time_of_impact_3d() {
        let left = Cuboid::new(10., 10., 10.);
        let left_start_transform = transform_3d(0., 0., 0., 0.);
        let left_end_transform = transform_3d(30., 0., 0., 0.);
        let right = Cuboid::new(10., 10., 10.);
        let right_transform = transform_3d(15., 0., 0., 0.);
        let gjk = GJK3::new();

        assert_ulps_eq!(
            0.1666667,
            gjk.intersect_time_of_impact(
                &left,
                &left_start_transform..&left_end_transform,
                &right,
                &right_transform..&right_transform
            ).unwrap()
        );

        assert_eq!(
            None,
            gjk.intersect_time_of_impact(
                &left,
                &left_start_transform..&left_start_transform,
                &right,
                &right_transform..&right_transform
            )
        );
    }
}
