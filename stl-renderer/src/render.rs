use std::collections::HashMap;

use image::{ImageBuffer, Rgba};

use crate::math::{transform_point, Mat4, Triangle, Vec3};

// ── Edge key for adjacency detection ────────────────────────────────────────

/// Quantise a float to an integer key (micrometre-level precision).
fn quantise(v: f32) -> i64 {
    (v * 1_000_000.0).round() as i64
}

/// Canonical, order-independent key for an edge defined by two 3-D points.
fn edge_key(a: Vec3, b: Vec3) -> ((i64, i64, i64), (i64, i64, i64)) {
    let ka = (quantise(a.x), quantise(a.y), quantise(a.z));
    let kb = (quantise(b.x), quantise(b.y), quantise(b.z));
    if ka <= kb {
        (ka, kb)
    } else {
        (kb, ka)
    }
}

// ── Rasterizer ──────────────────────────────────────────────────────────────

/// Per-edge info collected during the triangle loop.
struct EdgeInfo {
    /// Screen-space endpoints of the edge (x, y, ndc_z).
    screen: [(f32, f32, f32); 2],
    /// Face normals of triangles sharing this edge (max 2 relevant).
    normals: Vec<Vec3>,
}

pub fn render(
    triangles: &[Triangle],
    mvp: &Mat4,
    eye: Vec3,
    cam_target: Vec3,
    is_ortho: bool,
    width: u32,
    height: u32,
    color: [f32; 3],
    outline: bool,
    brightness: f32,
) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    let w = width as usize;
    let h = height as usize;
    let mut color_buf = vec![[0u8, 0, 0, 0]; w * h]; // transparent bg
    let mut depth_buf = vec![f32::INFINITY; w * h];

    let light_dir = Vec3::new(0.3, 0.8, 0.5).normalize();
    let light_dir2 = Vec3::new(-0.5, 0.3, -0.8).normalize();
    let brightness = brightness.max(0.0);

    // For orthographic projection the view rays are parallel, so use a
    // constant view direction (from target toward eye) for back-face culling.
    let ortho_view_dir = eye.sub(cam_target).normalize();

    // Edge adjacency map – only populated when outline is requested.
    let mut edge_map: HashMap<((i64, i64, i64), (i64, i64, i64)), EdgeInfo> = HashMap::new();

    for tri in triangles {
        let (x0, y0, z0, w0) = transform_point(mvp, tri.v0);
        let (x1, y1, z1, w1) = transform_point(mvp, tri.v1);
        let (x2, y2, z2, w2) = transform_point(mvp, tri.v2);

        if w0 <= 0.0 || w1 <= 0.0 || w2 <= 0.0 {
            continue;
        }

        let ndc0 = Vec3::new(x0 / w0, y0 / w0, z0 / w0);
        let ndc1 = Vec3::new(x1 / w1, y1 / w1, z1 / w1);
        let ndc2 = Vec3::new(x2 / w2, y2 / w2, z2 / w2);

        let sx0 = (ndc0.x + 1.0) * 0.5 * width as f32;
        let sy0 = (1.0 - ndc0.y) * 0.5 * height as f32;
        let sx1 = (ndc1.x + 1.0) * 0.5 * width as f32;
        let sy1 = (1.0 - ndc1.y) * 0.5 * height as f32;
        let sx2 = (ndc2.x + 1.0) * 0.5 * width as f32;
        let sy2 = (1.0 - ndc2.y) * 0.5 * height as f32;

        // Face normal from vertices (reliable)
        let edge1 = tri.v1.sub(tri.v0);
        let edge2 = tri.v2.sub(tri.v0);
        let face_normal = edge1.cross(edge2).normalize();

        // Back-face culling – use a constant direction for orthographic
        // (parallel rays) vs per-vertex direction for perspective.
        let view_dir = if is_ortho {
            ortho_view_dir
        } else {
            eye.sub(tri.v0).normalize()
        };
        if face_normal.dot(view_dir) < 0.0 {
            continue;
        }

        // Lighting – brightness scales the directional contribution
        let ndl1 = face_normal.dot(light_dir).max(0.0);
        let ndl2 = face_normal.dot(light_dir2).max(0.0);
        let ambient = 0.15;
        let intensity = (ambient + (0.55 * ndl1 + 0.35 * ndl2) * brightness).min(1.0);

        let r = (color[0] * intensity).min(255.0) as u8;
        let g = (color[1] * intensity).min(255.0) as u8;
        let b = (color[2] * intensity).min(255.0) as u8;

        // Collect edge adjacency info for the outline pass.
        // Mimics THREE.EdgesGeometry: only hard edges are drawn.
        if outline {
            let verts = [(tri.v0, sx0, sy0, ndc0.z), (tri.v1, sx1, sy1, ndc1.z), (tri.v2, sx2, sy2, ndc2.z)];
            for &(i, j) in &[(0usize, 1usize), (1, 2), (2, 0)] {
                let (va, sxa, sya, za) = verts[i];
                let (vb, sxb, syb, zb) = verts[j];
                let key = edge_key(va, vb);
                edge_map
                    .entry(key)
                    .and_modify(|info| {
                        info.normals.push(face_normal);
                    })
                    .or_insert_with(|| EdgeInfo {
                        screen: [(sxa, sya, za), (sxb, syb, zb)],
                        normals: vec![face_normal],
                    });
            }
        }

        // Bounding box
        let min_x = sx0.min(sx1).min(sx2).floor().max(0.0) as i32;
        let max_x = sx0.max(sx1).max(sx2).ceil().min(width as f32 - 1.0) as i32;
        let min_y = sy0.min(sy1).min(sy2).floor().max(0.0) as i32;
        let max_y = sy0.max(sy1).max(sy2).ceil().min(height as f32 - 1.0) as i32;

        let area = edge_function(sx0, sy0, sx1, sy1, sx2, sy2);
        if area.abs() < 1e-6 {
            continue;
        }
        let inv_area = 1.0 / area;

        for py in min_y..=max_y {
            for px in min_x..=max_x {
                let px_f = px as f32 + 0.5;
                let py_f = py as f32 + 0.5;

                let w0_e = edge_function(sx1, sy1, sx2, sy2, px_f, py_f);
                let w1_e = edge_function(sx2, sy2, sx0, sy0, px_f, py_f);
                let w2_e = edge_function(sx0, sy0, sx1, sy1, px_f, py_f);

                if w0_e >= 0.0 && w1_e >= 0.0 && w2_e >= 0.0 {
                    let bary0 = w0_e * inv_area;
                    let bary1 = w1_e * inv_area;
                    let bary2 = w2_e * inv_area;
                    let depth = bary0 * ndc0.z + bary1 * ndc1.z + bary2 * ndc2.z;
                    let idx = py as usize * w + px as usize;
                    if depth < depth_buf[idx] {
                        depth_buf[idx] = depth;
                        color_buf[idx] = [r, g, b, 255];
                    }
                }
            }
        }
    }

    // Outline pass – draw only hard / boundary edges (like THREE.EdgesGeometry).
    // Threshold: ~1 degree (same default as Three.js EdgesGeometry).
    if outline {
        let threshold_cos = 1.0f32.to_radians().cos(); // cos(1°) ≈ 0.99985
        for info in edge_map.values() {
            let draw = if info.normals.len() == 1 {
                // Boundary edge – always draw.
                true
            } else {
                // Shared edge – draw only when adjacent normals differ enough.
                let n0 = info.normals[0];
                let n1 = info.normals[1];
                n0.dot(n1) <= threshold_cos
            };
            if draw {
                draw_line(&mut color_buf, &depth_buf, w, h, info.screen[0], info.screen[1]);
            }
        }
    }

    let mut img = ImageBuffer::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let idx = y as usize * w + x as usize;
            let [r, g, b, a] = color_buf[idx];
            img.put_pixel(x, y, Rgba([r, g, b, a]));
        }
    }
    img
}

/// Depth-tested line drawing for outline edges (black, opaque).
/// Only draws pixels that are at or in front of the existing depth buffer,
/// so edges hidden behind the model are not visible.
fn draw_line(
    buf: &mut [[u8; 4]],
    depth_buf: &[f32],
    w: usize,
    h: usize,
    a: (f32, f32, f32),
    b: (f32, f32, f32),
) {
    let dx = (b.0 - a.0).abs();
    let dy = (b.1 - a.1).abs();
    let steps = dx.max(dy).ceil() as i32;
    if steps == 0 {
        return;
    }
    let inv = 1.0 / steps as f32;
    let xi = (b.0 - a.0) * inv;
    let yi = (b.1 - a.1) * inv;
    let zi = (b.2 - a.2) * inv;
    let (mut x, mut y, mut z) = (a.0, a.1, a.2);
    // Small bias so the edge isn't z-fighting with the surface it sits on.
    let depth_bias: f32 = -0.0005;
    for _ in 0..=steps {
        let px = x.round() as i32;
        let py = y.round() as i32;
        if px >= 0 && (px as usize) < w && py >= 0 && (py as usize) < h {
            let idx = py as usize * w + px as usize;
            if z + depth_bias <= depth_buf[idx] {
                buf[idx] = [0, 0, 0, 255];
            }
        }
        x += xi;
        y += yi;
        z += zi;
    }
}

fn edge_function(ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32) -> f32 {
    (cx - ax) * (by - ay) - (cy - ay) * (bx - ax)
}
