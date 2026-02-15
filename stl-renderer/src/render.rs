use image::{ImageBuffer, Rgba};

use crate::math::{transform_point, Mat4, Triangle, Vec3};

// ── Rasterizer ──────────────────────────────────────────────────────────────

pub fn render(
    triangles: &[Triangle],
    mvp: &Mat4,
    eye: Vec3,
    width: u32,
    height: u32,
    color: [f32; 3],
) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    let w = width as usize;
    let h = height as usize;
    let mut color_buf = vec![[0u8, 0, 0, 0]; w * h]; // transparent bg
    let mut depth_buf = vec![f32::INFINITY; w * h];

    let light_dir = Vec3::new(0.3, 0.8, 0.5).normalize();
    let light_dir2 = Vec3::new(-0.5, 0.3, -0.8).normalize();

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

        // Back-face culling
        let view_dir = eye.sub(tri.v0).normalize();
        if face_normal.dot(view_dir) < 0.0 {
            continue;
        }

        // Lighting
        let ndl1 = face_normal.dot(light_dir).max(0.0);
        let ndl2 = face_normal.dot(light_dir2).max(0.0);
        let ambient = 0.15;
        let intensity = (ambient + 0.55 * ndl1 + 0.35 * ndl2).min(1.0);

        let r = (color[0] * intensity).min(255.0) as u8;
        let g = (color[1] * intensity).min(255.0) as u8;
        let b = (color[2] * intensity).min(255.0) as u8;

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

fn edge_function(ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32) -> f32 {
    (cx - ax) * (by - ay) - (cy - ay) * (bx - ax)
}
