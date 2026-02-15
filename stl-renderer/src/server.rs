use actix_multipart::Multipart;
use actix_web::{web, HttpResponse};
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
    #[serde(default = "default_color")]
    color: String,
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
    "8ca0c8".to_string()
}
fn default_padding() -> u32 {
    10
}

fn parse_hex_color(hex: &str) -> [f32; 3] {
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6 {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&hex[0..2], 16),
            u8::from_str_radix(&hex[2..4], 16),
            u8::from_str_radix(&hex[4..6], 16),
        ) {
            return [r as f32, g as f32, b as f32];
        }
    }
    [140.0, 160.0, 200.0]
}

// ── Handlers ────────────────────────────────────────────────────────────────

#[actix_web::post("/render")]
pub async fn render_endpoint(
    query: web::Query<RenderParams>,
    mut payload: Multipart,
) -> HttpResponse {
    // Read the uploaded file and its filename
    let mut file_data: Vec<u8> = Vec::new();
    let mut filename = String::from("model.stl");
    while let Some(item) = payload.next().await {
        let mut field = match item {
            Ok(f) => f,
            Err(e) => {
                return HttpResponse::BadRequest()
                    .json(serde_json::json!({"error": format!("Multipart error: {}", e)}));
            }
        };
        // Only read data from file fields (those with a filename); skip text form fields
        let is_file_field = field
            .content_disposition()
            .and_then(|cd| cd.get_filename().map(|f| f.to_string()))
            .is_some();
        if let Some(cd) = field.content_disposition() {
            if let Some(fname) = cd.get_filename() {
                filename = fname.to_string();
            }
        }
        if !is_file_field {
            continue;
        }
        while let Some(chunk) = field.next().await {
            match chunk {
                Ok(data) => file_data.extend_from_slice(&data),
                Err(e) => {
                    return HttpResponse::BadRequest()
                        .json(serde_json::json!({"error": format!("Read error: {}", e)}));
                }
            }
        }
    }

    if file_data.is_empty() {
        return HttpResponse::BadRequest()
            .json(serde_json::json!({"error": "No file uploaded"}));
    }

    let (triangles, mesh_min, mesh_max) = match parse_model(&file_data, &filename) {
        Ok(m) => m,
        Err(e) => {
            return HttpResponse::BadRequest()
                .json(serde_json::json!({"error": format!("Parse error: {}", e)}));
        }
    };

    let params = query.into_inner();
    let width = params.width.clamp(1, 4096);
    let height = params.height.clamp(1, 4096);

    // Center mesh at origin
    let center = Vec3::new(
        (mesh_min.x + mesh_max.x) / 2.0,
        (mesh_min.y + mesh_max.y) / 2.0,
        (mesh_min.z + mesh_max.z) / 2.0,
    );

    // Apply Euler rotation (intrinsic XYZ, matching Three.js)
    let rot_x = params.rot_x.unwrap_or(0.0);
    let rot_y = params.rot_y.unwrap_or(0.0);
    let rot_z = params.rot_z.unwrap_or(0.0);
    let rot = rotation_matrix_xyz(rot_x, rot_y, rot_z);

    let transformed: Vec<Triangle> = triangles
        .iter()
        .map(|t| Triangle {
            v0: rotate_vec3(&rot, t.v0.sub(center)),
            v1: rotate_vec3(&rot, t.v1.sub(center)),
            v2: rotate_vec3(&rot, t.v2.sub(center)),
            normal: rotate_vec3(&rot, t.normal).normalize(),
        })
        .collect();

    // Compute AABB of the rotated mesh (rotation-dependent tight fit)
    let mut aabb_min = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
    let mut aabb_max = Vec3::new(f32::MIN, f32::MIN, f32::MIN);
    for t in &transformed {
        for v in [t.v0, t.v1, t.v2] {
            aabb_min.x = aabb_min.x.min(v.x);
            aabb_min.y = aabb_min.y.min(v.y);
            aabb_min.z = aabb_min.z.min(v.z);
            aabb_max.x = aabb_max.x.max(v.x);
            aabb_max.y = aabb_max.y.max(v.y);
            aabb_max.z = aabb_max.z.max(v.z);
        }
    }

    // AABB center — camera will target this point to center the object
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

    // Auto-fit: AABB-based, accounting for padding and centering
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
        for t in &transformed {
            for v in [t.v0, t.v1, t.v2] {
                let dz = v.z - aabb_cz;
                let d_h = dz + (v.x - aabb_cx).abs() / tan_h;
                let d_v = dz + (v.y - aabb_cy).abs() / tan_v;
                min_dist = min_dist.max(d_h).max(d_v);
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
    let color = parse_hex_color(&params.color);

    let outline = params.outline;
    let brightness = params.brightness.clamp(0.0, 5.0);
    let outline_thickness = params.outline_thickness.clamp(0.5, 10.0);

    log::info!(
        "Rendering: {}x{}, rot=({:.1},{:.1},{:.1}), proj={}, color={}, pad={}, outline={}, brightness={:.2}, outline_thickness={:.1}, {} triangles",
        width,
        height,
        rot_x,
        rot_y,
        rot_z,
        params.projection,
        params.color,
        padding,
        outline,
        brightness,
        outline_thickness,
        transformed.len()
    );

    let img = web::block(move || render(&transformed, &mvp, eye, cam_target, is_ortho, width, height, color, outline, brightness, outline_thickness))
        .await
        .unwrap();

    let mut png_data = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png_data);
    encoder
        .write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();

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
