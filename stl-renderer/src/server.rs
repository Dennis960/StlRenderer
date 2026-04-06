use actix_multipart::Multipart;
use actix_web::{web, HttpRequest, HttpResponse};
use futures_util::StreamExt;
use image::ImageEncoder;
use serde::Deserialize;

use crate::math::*;
use crate::parser::parse_model;
use crate::render::render;

// ── Parameters ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct RenderParams {
    #[serde(default = "default_width")]
    width: u32,
    #[serde(default = "default_height")]
    height: u32,
    #[serde(default)]
    rot_x: Option<f32>,
    #[serde(default)]
    rot_y: Option<f32>,
    #[serde(default)]
    rot_z: Option<f32>,
    #[serde(default = "default_fov")]
    fov: f32,
    #[serde(default = "default_projection")]
    projection: String,
    #[serde(default = "default_padding")]
    padding: u32,
    #[serde(default)]
    outline: bool,
    #[serde(default = "default_brightness")]
    brightness: f32,
    #[serde(default = "default_outline_thickness")]
    outline_thickness: f32,
}

fn default_outline_thickness() -> f32 {
    1.0
}

fn default_brightness() -> f32 {
    1.0
}

fn default_width() -> u32 {
    800
}
fn default_height() -> u32 {
    600
}
fn default_fov() -> f32 {
    45.0
}
fn default_projection() -> String {
    "perspective".to_string()
}
fn default_color() -> String {
    "cccccc".to_string()
}
fn default_padding() -> u32 {
    10
}

fn normalize_hex(mut hex: &str) -> &str {
    hex = hex.trim();
    if let Some(stripped) = hex.strip_prefix("%23") {
        hex = stripped;
    }
    if let Some(stripped) = hex.strip_prefix('#') {
        hex = stripped;
    }
    hex
}

fn parse_hex_triplet(hex: &str) -> Option<[f32; 3]> {
    if hex.len() != 6 {
        return None;
    }
    if let (Ok(r), Ok(g), Ok(b)) = (
        u8::from_str_radix(&hex[0..2], 16),
        u8::from_str_radix(&hex[2..4], 16),
        u8::from_str_radix(&hex[4..6], 16),
    ) {
        Some([r as f32, g as f32, b as f32])
    } else {
        None
    }
}

fn parse_hex_color(hex: &str) -> [f32; 3] {
    // Try the provided color first (supports raw, '#', and '%23' forms).
    let hex = normalize_hex(hex);
    if let Some(rgb) = parse_hex_triplet(hex) {
        return rgb;
    }

    // Fallback to the configured default hex from default_color().
    let def = default_color();
    let def_hex = normalize_hex(&def);
    if let Some(rgb) = parse_hex_triplet(def_hex) {
        return rgb;
    }

    // Final hard-coded fallback, should never be hit in normal use.
    [204.0, 204.0, 204.0]
}

// ── Handlers ────────────────────────────────────────────────────────────────

#[actix_web::post("/render")]
pub async fn render_endpoint(
    req: HttpRequest,
    query: web::Query<RenderParams>,
    mut payload: Multipart,
) -> HttpResponse {
    // Collect all uploaded file fields in order.
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    while let Some(item) = payload.next().await {
        let mut field = match item {
            Ok(f) => f,
            Err(e) => {
                return HttpResponse::BadRequest()
                    .json(serde_json::json!({"error": format!("Multipart error: {}", e)}));
            }
        };

        // Only process fields that carry a filename (i.e. file inputs).
        let filename = match field
            .content_disposition()
            .and_then(|cd| cd.get_filename().map(|f| f.to_string()))
        {
            Some(name) => name,
            None => {
                // Drain non-file fields to avoid stalling the multipart stream.
                while field.next().await.is_some() {}
                continue;
            }
        };

        let mut file_data: Vec<u8> = Vec::new();
        while let Some(chunk) = field.next().await {
            match chunk {
                Ok(data) => file_data.extend_from_slice(&data),
                Err(e) => {
                    return HttpResponse::BadRequest()
                        .json(serde_json::json!({"error": format!("Read error: {}", e)}));
                }
            }
        }
        files.push((filename, file_data));
    }

    if files.is_empty() {
        return HttpResponse::BadRequest()
            .json(serde_json::json!({"error": "No file uploaded"}));
    }

    let params = query.into_inner();
    let width = params.width.clamp(1, 4096);
    let height = params.height.clamp(1, 4096);

    // Extract all `color` query parameters in the order they appear.
    // Example: `?color=ff0000&color=00ff00`. If no colors are provided
    // at all, fall back to a built-in default. If fewer colors than
    // models are provided, reuse the last color.
    let mut raw_colors: Vec<String> = Vec::new();
    if let Some(qs) = req.uri().query() {
        for pair in qs.split('&') {
            if pair.is_empty() {
                continue;
            }
            let mut parts = pair.splitn(2, '=');
            let key = parts.next().unwrap_or("");
            if key != "color" {
                continue;
            }
            let value = parts.next().unwrap_or("");
            raw_colors.push(value.to_string());
        }
    }

    let fallback_color = raw_colors
        .last()
        .map(|c| parse_hex_color(c))
        .unwrap_or_else(|| parse_hex_color(&default_color()));
    let color_list: Vec<[f32; 3]> = raw_colors
        .iter()
        .map(|c| parse_hex_color(c))
        .collect();

    // Parse every uploaded file and accumulate a combined bounding box.
    let mut model_groups: Vec<(Vec<Triangle>, [f32; 3])> = Vec::new();
    let mut combined_min = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
    let mut combined_max = Vec3::new(f32::MIN, f32::MIN, f32::MIN);

    for (i, (filename, data)) in files.iter().enumerate() {
        let (triangles, mesh_min, mesh_max) = match parse_model(data, filename) {
            Ok(m) => m,
            Err(e) => {
                return HttpResponse::BadRequest()
                    .json(serde_json::json!({"error": format!("Parse error in '{}': {}", filename, e)}));
            }
        };

        combined_min.x = combined_min.x.min(mesh_min.x);
        combined_min.y = combined_min.y.min(mesh_min.y);
        combined_min.z = combined_min.z.min(mesh_min.z);
        combined_max.x = combined_max.x.max(mesh_max.x);
        combined_max.y = combined_max.y.max(mesh_max.y);
        combined_max.z = combined_max.z.max(mesh_max.z);

        let color = color_list.get(i).copied().unwrap_or(fallback_color);
        model_groups.push((triangles, color));
    }

    // Free uploaded file bytes — no longer needed after parsing.
    drop(files);

    // Center all models together using the combined bounding box so that their
    // relative positions are preserved while the group is placed at the origin.
    let center = Vec3::new(
        (combined_min.x + combined_max.x) / 2.0,
        (combined_min.y + combined_max.y) / 2.0,
        (combined_min.z + combined_max.z) / 2.0,
    );

    // Apply Euler rotation (intrinsic XYZ, matching Three.js) to the whole group.
    let rot_x = params.rot_x.unwrap_or(0.0);
    let rot_y = params.rot_y.unwrap_or(0.0);
    let rot_z = params.rot_z.unwrap_or(0.0);
    let rot = rotation_matrix_xyz(rot_x, rot_y, rot_z);

    for (triangles, _) in &mut model_groups {
        for t in triangles.iter_mut() {
            t.v0 = rotate_vec3(&rot, t.v0.sub(center));
            t.v1 = rotate_vec3(&rot, t.v1.sub(center));
            t.v2 = rotate_vec3(&rot, t.v2.sub(center));
            t.normal = rotate_vec3(&rot, t.normal).normalize();
        }
    }

    // Compute AABB of the rotated group (rotation-dependent tight fit).
    let mut aabb_min = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
    let mut aabb_max = Vec3::new(f32::MIN, f32::MIN, f32::MIN);
    for (triangles, _) in &model_groups {
        for t in triangles {
            for v in [t.v0, t.v1, t.v2] {
                aabb_min.x = aabb_min.x.min(v.x);
                aabb_min.y = aabb_min.y.min(v.y);
                aabb_min.z = aabb_min.z.min(v.z);
                aabb_max.x = aabb_max.x.max(v.x);
                aabb_max.y = aabb_max.y.max(v.y);
                aabb_max.z = aabb_max.z.max(v.z);
            }
        }
    }

    // AABB centre — camera will target this point to centre the object.
    let aabb_cx = (aabb_min.x + aabb_max.x) / 2.0;
    let aabb_cy = (aabb_min.y + aabb_max.y) / 2.0;
    let aabb_cz = (aabb_min.z + aabb_max.z) / 2.0;
    let half_x = ((aabb_max.x - aabb_min.x) / 2.0).max(0.001);
    let half_y = ((aabb_max.y - aabb_min.y) / 2.0).max(0.001);
    let half_z = ((aabb_max.z - aabb_min.z) / 2.0).max(0.001);

    let padding = params.padding.clamp(0, width.min(height) / 2 - 1);
    let pad_f = padding as f32;
    let aspect = width as f32 / height as f32;
    let is_ortho = params.projection == "orthographic";

    // Auto-fit: AABB-based, accounting for padding and centering.
    let (eye, cam_target, proj) = if is_ortho {
        let eff_w = (width as f32 - 2.0 * pad_f).max(1.0);
        let eff_h = (height as f32 - 2.0 * pad_f).max(1.0);
        let fhh_from_y = half_y * height as f32 / eff_h;
        let fhh_from_x = half_x * width as f32 / (eff_w * aspect);
        let fhh = fhh_from_y.max(fhh_from_x).max(0.001);
        let fhw = fhh * aspect;
        let dist = half_z + fhh.max(fhw) * 2.0;
        let near = 0.01_f32;
        let far = dist * 4.0;
        (
            Vec3::new(aabb_cx, aabb_cy, aabb_cz + dist),
            Vec3::new(aabb_cx, aabb_cy, aabb_cz),
            orthographic(-fhw, fhw, -fhh, fhh, near, far),
        )
    } else {
        let fov_rad = params.fov.clamp(1.0, 179.0).to_radians();
        let half_fov_v = fov_rad / 2.0;
        let half_fov_h = (aspect * half_fov_v.tan()).atan();
        let eff_half_fov_v = (half_fov_v.tan() * (height as f32 - 2.0 * pad_f).max(1.0) / height as f32).atan();
        let eff_half_fov_h = (half_fov_h.tan() * (width as f32 - 2.0 * pad_f).max(1.0) / width as f32).atan();
        let tan_h = eff_half_fov_h.tan();
        let tan_v = eff_half_fov_v.tan();
        let mut min_dist: f32 = 0.1;
        for (triangles, _) in &model_groups {
            for t in triangles {
                for v in [t.v0, t.v1, t.v2] {
                    let dz = v.z - aabb_cz;
                    let d_h = dz + (v.x - aabb_cx).abs() / tan_h;
                    let d_v = dz + (v.y - aabb_cy).abs() / tan_v;
                    min_dist = min_dist.max(d_h).max(d_v);
                }
            }
        }
        let dist = min_dist.max(half_z + 0.1);
        let near = (dist - half_z).max(0.01) * 0.5;
        let far = (dist + half_z) * 3.0;
        (
            Vec3::new(aabb_cx, aabb_cy, aabb_cz + dist),
            Vec3::new(aabb_cx, aabb_cy, aabb_cz),
            perspective(fov_rad, aspect, near, far),
        )
    };

    let up = Vec3::new(0.0, 1.0, 0.0);
    let view = look_at(eye, cam_target, up);

    let mvp = mat4_mul(&proj, &view);

    let outline = params.outline;
    let brightness = params.brightness.clamp(0.0, 5.0);
    let outline_thickness = params.outline_thickness.clamp(0.5, 10.0);

    let total_triangles: usize = model_groups.iter().map(|(t, _)| t.len()).sum();
    log::info!(
        "Rendering: {}x{}, models={}, rot=({:.1},{:.1},{:.1}), proj={}, pad={}, outline={}, brightness={:.2}, outline_thickness={:.1}, {} triangles total",
        width,
        height,
        model_groups.len(),
        rot_x,
        rot_y,
        rot_z,
        params.projection,
        padding,
        outline,
        brightness,
        outline_thickness,
        total_triangles,
    );

    let pixel_data = web::block(move || render(model_groups, mvp, eye, cam_target, is_ortho, width, height, outline, brightness, outline_thickness))
        .await
        .unwrap();

    let mut png_data = Vec::new();
    {
        let encoder = image::codecs::png::PngEncoder::new(&mut png_data);
        encoder
            .write_image(
                &pixel_data,
                width,
                height,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
    }
    // Free raw pixel data immediately after PNG encoding.
    drop(pixel_data);

    HttpResponse::Ok()
        .content_type("image/png")
        .body(png_data)
}

#[actix_web::get("/health")]
pub async fn health() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({"status": "ok"}))
}

#[actix_web::get("/")]
pub async fn index() -> HttpResponse {
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(include_str!("../static/index.html"))
}

#[actix_web::get("/style.css")]
pub async fn style_css() -> HttpResponse {
    HttpResponse::Ok()
        .content_type("text/css; charset=utf-8")
        .body(include_str!("../static/style.css"))
}

#[actix_web::get("/app.js")]
pub async fn app_js() -> HttpResponse {
    HttpResponse::Ok()
        .content_type("application/javascript; charset=utf-8")
        .body(include_str!("../static/app.js"))
}
