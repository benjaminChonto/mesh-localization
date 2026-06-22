use crate::state::MAX_SWARM_SIZE;
use esp_hal::rng::Rng;
use fixed::types::I16F16;
use heapless::Vec;
use shared::MdsResult;

// TODO: find highest acceptable value
const MDS_ITERATIONS: usize = 50;

#[derive(Default)]
pub struct MDS {
    X: MdsResult,
}

impl MDS {
    /**
     * Here we use unweighted version to avoid having to construct a pseudo-inverse matrix
     *
     * Basic outline:
     * - Initialize the result X randomly or use the previous result
     * - Calculate pairwise distances of the points in X
     * - Create n x n matrix B:
     *   (- distance in X / actual distance) for i!=j
     *   (- sum of row) for i == j to make sum of rows equal 0
     * - update X <- (1/n) * B * X
     * - substract mean from X (doesn't change end result, but keeps the values stable)
     *
     * SMACOF algorithm: <https://www.jstatsoft.org/article/view/v031i03>
     */
    pub async fn compute(
        &mut self,
        d: Vec<Vec<I16F16, MAX_SWARM_SIZE>, MAX_SWARM_SIZE>,
    ) -> &MdsResult {
        let D = make_symmetric(d);
        let n = D.len();
        if self.X.is_empty() || self.X.len() != n {
            self.X = initialize_mds(n);
        }

        for _ in 0..MDS_ITERATIONS {
            let dist = pairwise_distances(&self.X);
            let mut B: Vec<Vec<I16F16, MAX_SWARM_SIZE>, MAX_SWARM_SIZE> = Vec::new();

            for i in 0..n {
                let mut row: Vec<I16F16, MAX_SWARM_SIZE> = Vec::new();
                for j in 0..n {
                    let val = if i == j {
                        I16F16::ZERO
                    } else if dist[i][j] > I16F16::ZERO {
                        // Checked div returns None on overflow; fall back to 0
                        let scale = D[i][j].checked_div(dist[i][j]).unwrap_or(I16F16::ZERO);
                        -scale
                    } else {
                        I16F16::ZERO
                    };
                    let _ = row.push(val);
                }
                // Diagonal = negative row sum so each row sums to zero
                let off_diag_sum = row.iter().copied().fold(I16F16::ZERO, core::ops::Add::add);
                row[i] = -off_diag_sum;
                let _ = B.push(row);
            }

            let n_fixed = I16F16::from_num(n as i32);
            self.X = mat_mul(&B, &self.X)
                .iter()
                .map(|row| row.iter().copied().map(|e| e / n_fixed).collect())
                .collect();
            self.X = subtract_mean(&self.X);
            embassy_futures::yield_now().await;
        }
        &self.X
    }
}

fn make_symmetric(
    mut d: Vec<Vec<I16F16, MAX_SWARM_SIZE>, MAX_SWARM_SIZE>,
) -> Vec<Vec<I16F16, MAX_SWARM_SIZE>, MAX_SWARM_SIZE> {
    let n = d.len();
    for i in 0..n {
        for j in (i + 1)..n {
            // (a + b) >> 1 would divide by 2 but fixed-point >> is on the raw bits,
            // so use explicit division to get the correct semantic.
            let avg = (d[i][j] + d[j][i]) / I16F16::from_num(2);
            d[i][j] = avg;
            d[j][i] = avg;
        }
    }
    d
}

fn mat_mul(m1: &Vec<Vec<I16F16, MAX_SWARM_SIZE>, MAX_SWARM_SIZE>, m2: &MdsResult) -> MdsResult {
    let n = m1.len();
    let mut result = MdsResult::new();
    for i in 0..n {
        let mut row: Vec<I16F16, 2> = Vec::new();
        for j in 0..2 {
            let sum = (0..n).fold(I16F16::ZERO, |acc, k| acc + m1[i][k] * m2[k][j]);
            let _ = row.push(sum);
        }
        let _ = result.push(row);
    }
    result
}

fn initialize_mds(n: usize) -> MdsResult {
    let rng = Rng::new();
    let mut X = MdsResult::new();
    for _ in 0..n {
        // High 16 bits of random u32 → fractional part of I16F16, giving value in [0, 1)
        let x = I16F16::from_bits((rng.random() >> 16) as i32);
        let y = I16F16::from_bits((rng.random() >> 16) as i32);
        let _ = X.push(Vec::from_array([x, y]));
    }
    subtract_mean(&X)
}

fn subtract_mean(X: &MdsResult) -> MdsResult {
    let n = X.len();
    if n == 0 {
        return Vec::new();
    }
    let n_fixed = I16F16::from_num(n as i32);
    let mean_x = X
        .iter()
        .map(|row| row[0])
        .fold(I16F16::ZERO, core::ops::Add::add)
        / n_fixed;
    let mean_y = X
        .iter()
        .map(|row| row[1])
        .fold(I16F16::ZERO, core::ops::Add::add)
        / n_fixed;
    X.iter()
        .map(|row| Vec::from_array([row[0] - mean_x, row[1] - mean_y]))
        .collect()
}

fn pairwise_distances(X: &MdsResult) -> Vec<Vec<I16F16, MAX_SWARM_SIZE>, MAX_SWARM_SIZE> {
    let n = X.len();
    let mut dist = Vec::new();
    for i in 0..n {
        let mut row: Vec<I16F16, MAX_SWARM_SIZE> = Vec::new();
        for j in 0..n {
            let dx = X[i][0] - X[j][0];
            let dy = X[i][1] - X[j][1];
            let d = fixed_sqrt(dx * dx + dy * dy);
            let _ = row.push(d);
        }
        let _ = dist.push(row);
    }
    dist
}

// Integer square root for I16F16.
// For x with bits b (value = b/2^16), sqrt(x) has bits = isqrt(b << 16).
fn fixed_sqrt(x: I16F16) -> I16F16 {
    let b = x.to_bits();
    if b <= 0 {
        return I16F16::ZERO;
    }
    I16F16::from_bits(isqrt((b as i64) << 16) as i32)
}

fn isqrt(n: i64) -> i64 {
    if n <= 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}
