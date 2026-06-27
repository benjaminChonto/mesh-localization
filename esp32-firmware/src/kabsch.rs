//! 2D Kabsch alignment in fixed point.
//!
//! The Kabsch algorithm finds the rigid transform (rotation + translation, **no
//! scaling** — that would be Umeyama) that best maps one set of points onto
//! another in the least-squares sense. MDS only recovers a configuration up to
//! an arbitrary rotation/translation, so consecutive solutions can come out
//! spun around relative to each other. Aligning every new solution onto the
//! previous one with Kabsch keeps the orientation stable frame-to-frame.
//!
//! This is the proper-rotation form (determinant +1); reflections are **not**
//! applied, matching the classic Kabsch definition. Because each MDS solve
//! warm-starts from the previous (aligned) configuration it converges
//! continuously and does not spontaneously mirror, so rotation alone suffices.
//!
//! In 2D the optimal rotation has a closed form and needs no SVD: with the two
//! point sets centred on their centroids,
//!
//! ```text
//! A = Σ (pₓ·qₓ + p_y·q_y)
//! B = Σ (pₓ·q_y − p_y·qₓ)
//! θ = atan2(B, A)
//! ```
//!
//! maximises Σ qᵢ · R(θ) pᵢ, i.e. it is the rotation that best carries `p` onto
//! `q`. `atan2` and `sin`/`cos` are evaluated with CORDIC on the fixed-point
//! type.

use fixed::types::I16F16;
use heapless::Vec;
use shared::MdsResult;

/// Returns `current` rigidly transformed (rotated about its centroid, then
/// translated onto `reference`'s centroid) to best match `reference`.
///
/// If the two sets differ in length or are empty the input is returned
/// unchanged — there is nothing meaningful to align.
pub fn align(current: &MdsResult, reference: &MdsResult) -> MdsResult {
    let n = current.len();
    if n == 0 || n != reference.len() {
        return current.clone();
    }

    let (cur_cx, cur_cy) = centroid(current);
    let (ref_cx, ref_cy) = centroid(reference);

    let theta = optimal_rotation(current, reference, cur_cx, cur_cy, ref_cx, ref_cy);
    let (sin, cos) = cordic::sin_cos(theta);

    // out_i = R(θ) · (current_i − cur_centroid) + ref_centroid
    let mut out = MdsResult::new();
    for p in current {
        let dx = p[0] - cur_cx;
        let dy = p[1] - cur_cy;
        let rx = cos * dx - sin * dy + ref_cx;
        let ry = sin * dx + cos * dy + ref_cy;
        let _ = out.push(Vec::from_array([rx, ry]));
    }
    out
}

/// Centroid (mean x, mean y) of a point set. Caller guarantees it is non-empty.
fn centroid(points: &MdsResult) -> (I16F16, I16F16) {
    let n = I16F16::from_num(points.len() as i32);
    let sum_x = points
        .iter()
        .map(|p| p[0])
        .fold(I16F16::ZERO, core::ops::Add::add);
    let sum_y = points
        .iter()
        .map(|p| p[1])
        .fold(I16F16::ZERO, core::ops::Add::add);
    (sum_x / n, sum_y / n)
}

/// Optimal proper-rotation angle aligning `current` onto `reference`.
///
/// The cross-covariance sums `A` and `B` are accumulated in `i64` from the raw
/// fixed-point bits so the products cannot overflow `I16F16`. The angle only
/// depends on the *ratio* `B / A`, so both sums are scaled down by the same
/// power of two into a CORDIC-safe range before calling `atan2`.
fn optimal_rotation(
    current: &MdsResult,
    reference: &MdsResult,
    cur_cx: I16F16,
    cur_cy: I16F16,
    ref_cx: I16F16,
    ref_cy: I16F16,
) -> I16F16 {
    // a = Σ(pₓqₓ + p_yq_y), b = Σ(pₓq_y − p_yqₓ), scaled by 2^32 (raw bit products).
    let mut a: i64 = 0;
    let mut b: i64 = 0;
    for (p, q) in current.iter().zip(reference.iter()) {
        let px = (p[0] - cur_cx).to_bits() as i64;
        let py = (p[1] - cur_cy).to_bits() as i64;
        let qx = (q[0] - ref_cx).to_bits() as i64;
        let qy = (q[1] - ref_cy).to_bits() as i64;
        a += px * qx + py * qy;
        b += px * qy - py * qx;
    }

    if a == 0 && b == 0 {
        // Degenerate (e.g. all points coincident): no defined rotation.
        return I16F16::ZERO;
    }

    // Scale (a, b) down so the larger magnitude sits around 2^14 raw bits
    // (≈ 0.25), leaving headroom for CORDIC's intermediate growth. Shifting both
    // by the same amount preserves atan2's result.
    let max_mag = a.unsigned_abs().max(b.unsigned_abs());
    let shift = max_mag
        .checked_ilog2()
        .map_or(0, |bits| bits.saturating_sub(14));
    let x = I16F16::from_bits((a >> shift) as i32);
    let y = I16F16::from_bits((b >> shift) as i32);

    cordic::atan2(y, x)
}
