// ── Data types ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
    pub fn sub(self, o: Self) -> Self {
        Self::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
    pub fn cross(self, o: Self) -> Self {
        Self::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    pub fn dot(self, o: Self) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn scale(self, s: f32) -> Self {
        Self::new(self.x * s, self.y * s, self.z * s)
    }
    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }
    pub fn normalize(self) -> Self {
        let l = self.length();
        if l < 1e-10 {
            Self::new(0.0, 0.0, 0.0)
        } else {
            self.scale(1.0 / l)
        }
    }
}

pub struct Triangle {
    pub v0: Vec3,
    pub v1: Vec3,
    pub v2: Vec3,
    #[allow(dead_code)]
    pub normal: Vec3,
}

// ── Matrix math ─────────────────────────────────────────────────────────────

pub type Mat4 = [[f32; 4]; 4];

pub fn mat4_mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut r = [[0.0f32; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            for k in 0..4 {
                r[i][j] += a[i][k] * b[k][j];
            }
        }
    }
    r
}

pub fn look_at(eye: Vec3, target: Vec3, up: Vec3) -> Mat4 {
    let f = target.sub(eye).normalize();
    let s = f.cross(up).normalize();
    let u = s.cross(f);
    [
        [s.x, s.y, s.z, -s.dot(eye)],
        [u.x, u.y, u.z, -u.dot(eye)],
        [-f.x, -f.y, -f.z, f.dot(eye)],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

pub fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let f = 1.0 / (fov_y / 2.0).tan();
    [
        [f / aspect, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [
            0.0,
            0.0,
            (far + near) / (near - far),
            2.0 * far * near / (near - far),
        ],
        [0.0, 0.0, -1.0, 0.0],
    ]
}

pub fn orthographic(left: f32, right: f32, bottom: f32, top: f32, near: f32, far: f32) -> Mat4 {
    let rl = right - left;
    let tb = top - bottom;
    let fn_ = far - near;
    [
        [2.0 / rl, 0.0, 0.0, -(right + left) / rl],
        [0.0, 2.0 / tb, 0.0, -(top + bottom) / tb],
        [0.0, 0.0, -2.0 / fn_, -(far + near) / fn_],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

/// Intrinsic XYZ Euler rotation matrix (matches Three.js Euler order 'XYZ').
/// Three.js 'XYZ' intrinsic = extrinsic Z-Y-X → M = Rx · Ry · Rz
pub fn rotation_matrix_xyz(rx_deg: f32, ry_deg: f32, rz_deg: f32) -> Mat4 {
    let (rx, ry, rz) = (
        rx_deg.to_radians(),
        ry_deg.to_radians(),
        rz_deg.to_radians(),
    );
    let (sx, cx) = (rx.sin(), rx.cos());
    let (sy, cy) = (ry.sin(), ry.cos());
    let (sz, cz) = (rz.sin(), rz.cos());
    [
        [cy * cz, -cy * sz, sy, 0.0],
        [
            cx * sz + sx * sy * cz,
            cx * cz - sx * sy * sz,
            -sx * cy,
            0.0,
        ],
        [
            sx * sz - cx * sy * cz,
            sx * cz + cx * sy * sz,
            cx * cy,
            0.0,
        ],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

pub fn rotate_vec3(m: &Mat4, v: Vec3) -> Vec3 {
    Vec3::new(
        m[0][0] * v.x + m[0][1] * v.y + m[0][2] * v.z,
        m[1][0] * v.x + m[1][1] * v.y + m[1][2] * v.z,
        m[2][0] * v.x + m[2][1] * v.y + m[2][2] * v.z,
    )
}

pub fn transform_point(m: &Mat4, p: Vec3) -> (f32, f32, f32, f32) {
    let x = m[0][0] * p.x + m[0][1] * p.y + m[0][2] * p.z + m[0][3];
    let y = m[1][0] * p.x + m[1][1] * p.y + m[1][2] * p.z + m[1][3];
    let z = m[2][0] * p.x + m[2][1] * p.y + m[2][2] * p.z + m[2][3];
    let w = m[3][0] * p.x + m[3][1] * p.y + m[3][2] * p.z + m[3][3];
    (x, y, z, w)
}
