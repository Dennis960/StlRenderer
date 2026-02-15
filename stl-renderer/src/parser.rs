use byteorder::{LittleEndian, ReadBytesExt};
use serde::Deserialize;
use std::io::{BufRead, BufReader, Cursor};

use crate::math::{Triangle, Vec3};

/// Unified parse entry point — detects format from filename extension and file content.
pub fn parse_model(data: &[u8], filename: &str) -> Result<(Vec<Triangle>, Vec3, Vec3), String> {
    let ext = filename
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "glb" | "gltf" => parse_glb(data),
        "obj" => parse_obj(data),
        "tjs" | "json" => parse_tjs(data),
        "stl" => parse_stl(data),
        _ => {
            // Try auto-detect: GLB magic, JSON/TJS, then STL
            if data.len() >= 4 && &data[0..4] == b"glTF" {
                parse_glb(data)
            } else if data.iter().position(|&b| !b.is_ascii_whitespace()).map_or(false, |p| data[p] == b'{') {
                parse_tjs(data)
            } else {
                parse_stl(data)
            }
        }
    }
}

// ── STL Parser (binary + ASCII) ─────────────────────────────────────────────

fn parse_stl(data: &[u8]) -> Result<(Vec<Triangle>, Vec3, Vec3), String> {
    // Detect ASCII STL: starts with "solid" (but not followed by binary header that happens to match)
    let is_ascii = data.len() > 5
        && data[0..5] == *b"solid"
        && (data[5] == b' ' || data[5] == b'\n' || data[5] == b'\r' || data[5] == b'\t');

    // Extra check: binary STL could start with "solid" in the header.
    // If it's ASCII, it should contain "facet" somewhere in the first ~1000 bytes.
    if is_ascii {
        let peek = &data[..data.len().min(1000)];
        if peek.windows(5).any(|w| w == b"facet") {
            return parse_ascii_stl(data);
        }
    }

    parse_binary_stl(data)
}

fn parse_binary_stl(data: &[u8]) -> Result<(Vec<Triangle>, Vec3, Vec3), String> {
    if data.len() < 84 {
        return Err("File too small for binary STL".into());
    }

    let mut cursor = Cursor::new(data);
    cursor.set_position(80);

    let num_triangles = cursor
        .read_u32::<LittleEndian>()
        .map_err(|e| format!("Failed to read triangle count: {}", e))?;

    let expected_size = 84 + num_triangles as usize * 50;
    if data.len() < expected_size {
        return Err(format!(
            "File too small: expected {} bytes, got {}",
            expected_size,
            data.len()
        ));
    }

    let mut triangles = Vec::with_capacity(num_triangles as usize);
    let mut min = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
    let mut max = Vec3::new(f32::MIN, f32::MIN, f32::MIN);

    for _ in 0..num_triangles {
        let nx = cursor.read_f32::<LittleEndian>().unwrap();
        let ny = cursor.read_f32::<LittleEndian>().unwrap();
        let nz = cursor.read_f32::<LittleEndian>().unwrap();

        let mut verts = [Vec3::new(0.0, 0.0, 0.0); 3];
        for v in verts.iter_mut() {
            let x = cursor.read_f32::<LittleEndian>().unwrap();
            let y = cursor.read_f32::<LittleEndian>().unwrap();
            let z = cursor.read_f32::<LittleEndian>().unwrap();
            *v = Vec3::new(x, y, z);

            min.x = min.x.min(x);
            min.y = min.y.min(y);
            min.z = min.z.min(z);
            max.x = max.x.max(x);
            max.y = max.y.max(y);
            max.z = max.z.max(z);
        }

        let _ = cursor.read_u16::<LittleEndian>();

        triangles.push(Triangle {
            v0: verts[0],
            v1: verts[1],
            v2: verts[2],
            normal: Vec3::new(nx, ny, nz),
        });
    }

    Ok((triangles, min, max))
}

fn parse_ascii_stl(data: &[u8]) -> Result<(Vec<Triangle>, Vec3, Vec3), String> {
    let reader = BufReader::new(Cursor::new(data));
    let mut triangles = Vec::new();
    let mut min = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
    let mut max = Vec3::new(f32::MIN, f32::MIN, f32::MIN);

    let mut normal = Vec3::new(0.0, 0.0, 0.0);
    let mut verts: Vec<Vec3> = Vec::new();

    for line in reader.lines() {
        let line = line.map_err(|e| format!("Read error: {}", e))?;
        let line = line.trim();

        if let Some(rest) = line.strip_prefix("facet normal") {
            let parts: Vec<f32> = rest
                .split_whitespace()
                .filter_map(|s| s.parse().ok())
                .collect();
            if parts.len() == 3 {
                normal = Vec3::new(parts[0], parts[1], parts[2]);
            }
            verts.clear();
        } else if let Some(rest) = line.strip_prefix("vertex") {
            let parts: Vec<f32> = rest
                .split_whitespace()
                .filter_map(|s| s.parse().ok())
                .collect();
            if parts.len() == 3 {
                let v = Vec3::new(parts[0], parts[1], parts[2]);
                min.x = min.x.min(v.x);
                min.y = min.y.min(v.y);
                min.z = min.z.min(v.z);
                max.x = max.x.max(v.x);
                max.y = max.y.max(v.y);
                max.z = max.z.max(v.z);
                verts.push(v);
            }
        } else if line.starts_with("endfacet") && verts.len() >= 3 {
            triangles.push(Triangle {
                v0: verts[0],
                v1: verts[1],
                v2: verts[2],
                normal,
            });
        }
    }

    if triangles.is_empty() {
        return Err("No triangles found in ASCII STL".into());
    }

    Ok((triangles, min, max))
}

// ── GLB/glTF Parser ─────────────────────────────────────────────────────────

fn parse_glb(data: &[u8]) -> Result<(Vec<Triangle>, Vec3, Vec3), String> {
    let (document, buffers, _images) =
        gltf::import_slice(data).map_err(|e| format!("glTF parse error: {}", e))?;

    let mut triangles = Vec::new();
    let mut min = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
    let mut max = Vec3::new(f32::MIN, f32::MIN, f32::MIN);

    for mesh in document.meshes() {
        for primitive in mesh.primitives() {
            let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));

            let positions: Vec<[f32; 3]> = match reader.read_positions() {
                Some(iter) => iter.collect(),
                None => continue,
            };

            // Read normals if available, otherwise we'll compute them
            let normals: Option<Vec<[f32; 3]>> =
                reader.read_normals().map(|iter| iter.collect());

            if let Some(indices_reader) = reader.read_indices() {
                let indices: Vec<u32> = indices_reader.into_u32().collect();
                for chunk in indices.chunks(3) {
                    if chunk.len() < 3 {
                        continue;
                    }
                    let (i0, i1, i2) = (chunk[0] as usize, chunk[1] as usize, chunk[2] as usize);
                    if i0 >= positions.len() || i1 >= positions.len() || i2 >= positions.len() {
                        continue;
                    }

                    let v0 = Vec3::new(positions[i0][0], positions[i0][1], positions[i0][2]);
                    let v1 = Vec3::new(positions[i1][0], positions[i1][1], positions[i1][2]);
                    let v2 = Vec3::new(positions[i2][0], positions[i2][1], positions[i2][2]);

                    let normal = if let Some(ref norms) = normals {
                        Vec3::new(norms[i0][0], norms[i0][1], norms[i0][2])
                    } else {
                        let e1 = v1.sub(v0);
                        let e2 = v2.sub(v0);
                        e1.cross(e2).normalize()
                    };

                    update_bounds(&v0, &mut min, &mut max);
                    update_bounds(&v1, &mut min, &mut max);
                    update_bounds(&v2, &mut min, &mut max);

                    triangles.push(Triangle { v0, v1, v2, normal });
                }
            } else {
                // Non-indexed geometry
                for chunk in positions.chunks(3) {
                    if chunk.len() < 3 {
                        continue;
                    }
                    let v0 = Vec3::new(chunk[0][0], chunk[0][1], chunk[0][2]);
                    let v1 = Vec3::new(chunk[1][0], chunk[1][1], chunk[1][2]);
                    let v2 = Vec3::new(chunk[2][0], chunk[2][1], chunk[2][2]);

                    let normal = if let Some(ref norms) = normals {
                        Vec3::new(norms[0][0], norms[0][1], norms[0][2])
                    } else {
                        let e1 = v1.sub(v0);
                        let e2 = v2.sub(v0);
                        e1.cross(e2).normalize()
                    };

                    update_bounds(&v0, &mut min, &mut max);
                    update_bounds(&v1, &mut min, &mut max);
                    update_bounds(&v2, &mut min, &mut max);

                    triangles.push(Triangle { v0, v1, v2, normal });
                }
            }
        }
    }

    if triangles.is_empty() {
        return Err("No triangles found in glTF/GLB file".into());
    }

    Ok((triangles, min, max))
}

// ── OBJ Parser ──────────────────────────────────────────────────────────────

fn parse_obj(data: &[u8]) -> Result<(Vec<Triangle>, Vec3, Vec3), String> {
    let mut reader = BufReader::new(Cursor::new(data));
    let (models, _materials) = tobj::load_obj_buf(
        &mut reader,
        &tobj::LoadOptions {
            triangulate: true,
            single_index: true,
            ..Default::default()
        },
        |_mtl_path| {
            // We don't load material files — just return empty
            Ok((vec![], ahash::AHashMap::new()))
        },
    )
    .map_err(|e| format!("OBJ parse error: {}", e))?;

    let mut triangles = Vec::new();
    let mut min = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
    let mut max = Vec3::new(f32::MIN, f32::MIN, f32::MIN);

    for model in &models {
        let mesh = &model.mesh;
        let positions = &mesh.positions;
        let normals = &mesh.normals;
        let indices = &mesh.indices;

        for chunk in indices.chunks(3) {
            if chunk.len() < 3 {
                continue;
            }
            let (i0, i1, i2) = (chunk[0] as usize, chunk[1] as usize, chunk[2] as usize);

            let v0 = Vec3::new(
                positions[i0 * 3],
                positions[i0 * 3 + 1],
                positions[i0 * 3 + 2],
            );
            let v1 = Vec3::new(
                positions[i1 * 3],
                positions[i1 * 3 + 1],
                positions[i1 * 3 + 2],
            );
            let v2 = Vec3::new(
                positions[i2 * 3],
                positions[i2 * 3 + 1],
                positions[i2 * 3 + 2],
            );

            let normal = if normals.len() >= (i0 + 1) * 3
                && normals.len() >= (i1 + 1) * 3
                && normals.len() >= (i2 + 1) * 3
            {
                Vec3::new(normals[i0 * 3], normals[i0 * 3 + 1], normals[i0 * 3 + 2])
            } else {
                let e1 = v1.sub(v0);
                let e2 = v2.sub(v0);
                e1.cross(e2).normalize()
            };

            update_bounds(&v0, &mut min, &mut max);
            update_bounds(&v1, &mut min, &mut max);
            update_bounds(&v2, &mut min, &mut max);

            triangles.push(Triangle { v0, v1, v2, normal });
        }
    }

    if triangles.is_empty() {
        return Err("No triangles found in OBJ file".into());
    }

    Ok((triangles, min, max))
}

// ── TJS (Three.js JSON) Parser ──────────────────────────────────────────────

// ── Typed deserialization structs (avoids massive serde_json::Value DOM) ─────

#[derive(Deserialize)]
struct TjsFile {
    #[serde(default = "default_scale")]
    scale: f32,
    #[serde(default)]
    vertices: Vec<f32>,
    #[serde(default)]
    faces: Vec<i64>,
    #[serde(default)]
    normals: Vec<f32>,
    #[serde(default)]
    uvs: Vec<Vec<f32>>,
    #[serde(default)]
    geometries: Vec<TjsGeometry>,
}

fn default_scale() -> f32 {
    1.0
}

#[derive(Deserialize)]
struct TjsGeometry {
    #[serde(default)]
    data: Option<TjsGeomData>,
}

#[derive(Deserialize)]
struct TjsGeomData {
    #[serde(default)]
    attributes: Option<TjsAttributes>,
    #[serde(default)]
    index: Option<TjsIndexArray>,
}

#[derive(Deserialize)]
struct TjsAttributes {
    #[serde(default)]
    position: Option<TjsTypedArray<f32>>,
    #[serde(default)]
    normal: Option<TjsTypedArray<f32>>,
}

#[derive(Deserialize)]
struct TjsTypedArray<T> {
    #[serde(default)]
    array: Vec<T>,
}

#[derive(Deserialize)]
struct TjsIndexArray {
    #[serde(default)]
    array: Vec<u32>,
}

fn parse_tjs(data: &[u8]) -> Result<(Vec<Triangle>, Vec3, Vec3), String> {
    let tjs: TjsFile =
        serde_json::from_slice(data).map_err(|e| format!("Invalid JSON: {}", e))?;

    let mut triangles = Vec::new();
    let mut min = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
    let mut max = Vec3::new(f32::MIN, f32::MIN, f32::MIN);

    // ── Modern BufferGeometry format (used by CadQuery TJS export) ───────
    for geom in &tjs.geometries {
        let data_obj = match &geom.data {
            Some(d) => d,
            None => continue,
        };
        let attrs = match &data_obj.attributes {
            Some(a) => a,
            None => continue,
        };

        let positions = match &attrs.position {
            Some(p) => &p.array,
            None => continue,
        };

        let normals: &[f32] = attrs
            .normal
            .as_ref()
            .map(|n| n.array.as_slice())
            .unwrap_or(&[]);

        let indices: Option<&[u32]> = data_obj
            .index
            .as_ref()
            .map(|idx| idx.array.as_slice());

        build_triangles_from_buffers(
            positions,
            normals,
            indices,
            &mut triangles,
            &mut min,
            &mut max,
        );
    }

    // ── Legacy Three.js Geometry format (top-level vertices / faces) ─────
    if triangles.is_empty() && !tjs.vertices.is_empty() && !tjs.faces.is_empty() {
        let num_uv_layers = tjs.uvs.len();

        parse_legacy_faces(
            &tjs.vertices,
            &tjs.normals,
            &tjs.faces,
            num_uv_layers,
            &mut triangles,
            &mut min,
            &mut max,
        );
    }

    // Free the parsed JSON data now that triangles are built
    drop(tjs);

    if triangles.is_empty() {
        return Err("No triangles found in TJS/JSON file".into());
    }

    Ok((triangles, min, max))
}

/// Build triangles from flat position / normal / index buffers (BufferGeometry).
fn build_triangles_from_buffers(
    positions: &[f32],
    normals: &[f32],
    indices: Option<&[u32]>,
    triangles: &mut Vec<Triangle>,
    min: &mut Vec3,
    max: &mut Vec3,
) {
    if let Some(indices) = indices {
        for chunk in indices.chunks(3) {
            if chunk.len() < 3 {
                continue;
            }
            let (i0, i1, i2) = (chunk[0] as usize, chunk[1] as usize, chunk[2] as usize);
            if i0 * 3 + 2 >= positions.len()
                || i1 * 3 + 2 >= positions.len()
                || i2 * 3 + 2 >= positions.len()
            {
                continue;
            }

            let v0 = Vec3::new(positions[i0 * 3], positions[i0 * 3 + 1], positions[i0 * 3 + 2]);
            let v1 = Vec3::new(positions[i1 * 3], positions[i1 * 3 + 1], positions[i1 * 3 + 2]);
            let v2 = Vec3::new(positions[i2 * 3], positions[i2 * 3 + 1], positions[i2 * 3 + 2]);

            let normal = if i0 * 3 + 2 < normals.len() {
                Vec3::new(normals[i0 * 3], normals[i0 * 3 + 1], normals[i0 * 3 + 2])
            } else {
                let e1 = v1.sub(v0);
                let e2 = v2.sub(v0);
                e1.cross(e2).normalize()
            };

            update_bounds(&v0, min, max);
            update_bounds(&v1, min, max);
            update_bounds(&v2, min, max);
            triangles.push(Triangle { v0, v1, v2, normal });
        }
    } else {
        // Non-indexed: every 3 vertices (9 floats) = 1 triangle
        let vert_count = positions.len() / 3;
        let tri_count = vert_count / 3;
        for t in 0..tri_count {
            let b0 = t * 9;
            let b1 = b0 + 3;
            let b2 = b0 + 6;

            let v0 = Vec3::new(positions[b0], positions[b0 + 1], positions[b0 + 2]);
            let v1 = Vec3::new(positions[b1], positions[b1 + 1], positions[b1 + 2]);
            let v2 = Vec3::new(positions[b2], positions[b2 + 1], positions[b2 + 2]);

            let normal = if b0 + 2 < normals.len() {
                Vec3::new(normals[b0], normals[b0 + 1], normals[b0 + 2])
            } else {
                let e1 = v1.sub(v0);
                let e2 = v2.sub(v0);
                e1.cross(e2).normalize()
            };

            update_bounds(&v0, min, max);
            update_bounds(&v1, min, max);
            update_bounds(&v2, min, max);
            triangles.push(Triangle { v0, v1, v2, normal });
        }
    }
}

/// Parse the legacy Three.js face-array format with per-face bitmask encoding.
fn parse_legacy_faces(
    vertices: &[f32],
    normals_arr: &[f32],
    faces: &[i64],
    num_uv_layers: usize,
    triangles: &mut Vec<Triangle>,
    min: &mut Vec3,
    max: &mut Vec3,
) {
    let mut i = 0;
    while i < faces.len() {
        let bitmask = faces[i] as u32;
        i += 1;

        let is_quad = (bitmask & 1) != 0;
        let has_material = (bitmask & 2) != 0;
        let has_face_uv = (bitmask & 4) != 0;
        let has_vert_uv = (bitmask & 8) != 0;
        let has_face_norm = (bitmask & 16) != 0;
        let has_vert_norm = (bitmask & 32) != 0;
        let has_face_color = (bitmask & 64) != 0;
        let has_vert_color = (bitmask & 128) != 0;

        let nv = if is_quad { 4usize } else { 3usize };

        // Vertex indices
        if i + nv > faces.len() {
            break;
        }
        let vi: Vec<usize> = faces[i..i + nv].iter().map(|&x| x as usize).collect();
        i += nv;

        // Skip material index
        if has_material {
            i += 1;
        }
        // Skip face UV (deprecated)
        if has_face_uv {
            i += num_uv_layers;
        }
        // Skip face-vertex UVs
        if has_vert_uv {
            i += num_uv_layers * nv;
        }

        // Face normal index
        let face_ni = if has_face_norm {
            let n = faces.get(i).copied().unwrap_or(0) as usize;
            i += 1;
            Some(n)
        } else {
            None
        };

        // Vertex normal indices
        let vert_ni: Option<Vec<usize>> = if has_vert_norm {
            if i + nv > faces.len() {
                break;
            }
            let indices: Vec<usize> = faces[i..i + nv].iter().map(|&x| x as usize).collect();
            i += nv;
            Some(indices)
        } else {
            None
        };

        // Skip face color
        if has_face_color {
            i += 1;
        }
        // Skip vertex colors
        if has_vert_color {
            i += nv;
        }

        // Build vertex positions
        let mut v: Vec<Vec3> = Vec::with_capacity(nv);
        for &idx in &vi {
            if idx * 3 + 2 < vertices.len() {
                v.push(Vec3::new(
                    vertices[idx * 3],
                    vertices[idx * 3 + 1],
                    vertices[idx * 3 + 2],
                ));
            }
        }
        if v.len() < 3 {
            continue;
        }

        // Resolve normal for a given vertex slot
        let resolve_normal = |slot: usize| -> Vec3 {
            if let Some(ref vni) = vert_ni {
                if slot < vni.len() {
                    let ni = vni[slot];
                    if ni * 3 + 2 < normals_arr.len() {
                        return Vec3::new(
                            normals_arr[ni * 3],
                            normals_arr[ni * 3 + 1],
                            normals_arr[ni * 3 + 2],
                        );
                    }
                }
            }
            if let Some(ni) = face_ni {
                if ni * 3 + 2 < normals_arr.len() {
                    return Vec3::new(
                        normals_arr[ni * 3],
                        normals_arr[ni * 3 + 1],
                        normals_arr[ni * 3 + 2],
                    );
                }
            }
            let e1 = v[1].sub(v[0]);
            let e2 = v[2].sub(v[0]);
            e1.cross(e2).normalize()
        };

        // Triangle 1: v[0], v[1], v[2]
        let normal = resolve_normal(0);
        update_bounds(&v[0], min, max);
        update_bounds(&v[1], min, max);
        update_bounds(&v[2], min, max);
        triangles.push(Triangle {
            v0: v[0],
            v1: v[1],
            v2: v[2],
            normal,
        });

        // Triangle 2 for quads: v[0], v[2], v[3]
        if is_quad && v.len() >= 4 {
            let normal = resolve_normal(0);
            update_bounds(&v[3], min, max);
            triangles.push(Triangle {
                v0: v[0],
                v1: v[2],
                v2: v[3],
                normal,
            });
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn update_bounds(v: &Vec3, min: &mut Vec3, max: &mut Vec3) {
    min.x = min.x.min(v.x);
    min.y = min.y.min(v.y);
    min.z = min.z.min(v.z);
    max.x = max.x.max(v.x);
    max.y = max.y.max(v.y);
    max.z = max.z.max(v.z);
}
