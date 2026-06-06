use crate::state::MAX_SWARM_SIZE;
use esp_hal::rng::Rng;
use heapless::Vec;

// TODO: find highest acceptable value
const MDS_ITERATIONS: usize = 10;

#[derive(Default)]
pub struct MDS {
    X: Vec<Vec<f32, 2>, MAX_SWARM_SIZE>,
}

impl MDS {
    /**
     * SMACOF algorithm: <https://www.jstatsoft.org/article/view/v031i03>
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
     */
    pub fn compute(
        &mut self,
        d: Vec<Vec<f32, MAX_SWARM_SIZE>, MAX_SWARM_SIZE>,
    ) -> &Vec<Vec<f32, 2>, MAX_SWARM_SIZE> {
        let D = make_symmetric(d);
        let n: usize = D.len();
        if self.X.is_empty() || self.X.len() != n {
            self.X = initialize_mds(n);
        }

        for _ in 0..MDS_ITERATIONS {
            let dist = pairwise_distances(&self.X);
            let mut B = Vec::<Vec<f32, MAX_SWARM_SIZE>, MAX_SWARM_SIZE>::new();
            for i in 0..n {
                let mut row = Vec::<f32, MAX_SWARM_SIZE>::new();
                for j in 0..n {
                    if i == j {
                        let _ = row.push(0.0);
                    } else {
                        if dist[i][j] > 1e-6 {
                            let scale = -D[i][j] / dist[i][j];
                            let _ = row.push(scale);
                        } else {
                            let _ = row.push(0.0);
                        }
                    } else {
                        let _ = row.push(0.0);
                    }
                }
                row[i] = -row.iter().sum::<f32>();
                let _ = B.push(row);
            }

            self.X = mat_mul(&B, &self.X)
                .iter()
                .map(|row| row.iter().map(|e| e / n as f32).collect())
                .collect();
            self.X = substract_mean(&self.X);
        }
        &self.X
    }
}

fn make_symmetric(
    mut d: Vec<Vec<f32, MAX_SWARM_SIZE>, MAX_SWARM_SIZE>,
) -> Vec<Vec<f32, MAX_SWARM_SIZE>, MAX_SWARM_SIZE> {
    let n = d.len();

    for i in 0..n {
        for j in (i + 1)..n {
            let avg = (d[i][j] + d[j][i]) * 0.5;
            d[i][j] = avg;
            d[j][i] = avg;
        }
    }
    d
}

fn mat_mul(
    m1: &Vec<Vec<f32, MAX_SWARM_SIZE>, MAX_SWARM_SIZE>,
    m2: &Vec<Vec<f32, 2>, MAX_SWARM_SIZE>,
) -> Vec<Vec<f32, 2>, MAX_SWARM_SIZE> {
    let n = m1.len();
    let mut result = Vec::new();

    for i in 0..n {
        let mut row = Vec::new();

        for j in 0..2 {
            let mut sum = 0.0;
            for k in 0..n {
                sum += m1[i][k] * m2[k][j];
            }
            let _ = row.push(sum);
        }
        let _ = result.push(row);
    }
    result
}

fn initialize_mds(n: usize) -> Vec<Vec<f32, 2>, MAX_SWARM_SIZE> {
    let rng = Rng::new();
    let mut X = Vec::<Vec<f32, 2>, MAX_SWARM_SIZE>::new();
    for _ in 0..n {
        let x = (rng.random() as f32) / (u32::MAX as f32);
        let y = (rng.random() as f32) / (u32::MAX as f32);
        let _ = X.push(Vec::from_array([x, y]));
    }
    substract_mean(&X)
}

fn substract_mean(X: &Vec<Vec<f32, 2>, MAX_SWARM_SIZE>) -> Vec<Vec<f32, 2>, MAX_SWARM_SIZE> {
    let n = X.len();
    if n == 0 {
        return Vec::new();
    }
    let mean_x = X.iter().map(|row| row[0]).sum::<f32>() / n as f32;
    let mean_y = X.iter().map(|row| row[1]).sum::<f32>() / n as f32;

    X.iter()
        .map(|row| Vec::from_array([row[0] - mean_x, row[1] - mean_y]))
        .collect()
}

fn pairwise_distances(
    X: &Vec<Vec<f32, 2>, MAX_SWARM_SIZE>,
) -> Vec<Vec<f32, MAX_SWARM_SIZE>, MAX_SWARM_SIZE> {
    let n = X.len();
    let mut dist = Vec::new();

    for i in 0..n {
        let mut row = Vec::new();

        for j in 0..n {
            let dx = X[i][0] - X[j][0];
            let dy = X[i][1] - X[j][1];
            row.push(libm::sqrtf(dx * dx + dy * dy)).unwrap();
        }
        let _ = dist.push(row);
    }
    dist
}
