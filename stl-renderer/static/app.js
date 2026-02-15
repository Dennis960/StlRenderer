import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import { TransformControls } from 'three/addons/controls/TransformControls.js';
import { STLLoader } from 'three/addons/loaders/STLLoader.js';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import { OBJLoader } from 'three/addons/loaders/OBJLoader.js';

const $ = id => document.getElementById(id);

// ── State ──────────────────────────────────
let modelFile = null;
let currentMesh = null;
let gizmo = null;
let aabb = { cx: 0, cy: 0, cz: 0, halfX: 1, halfY: 1, halfZ: 1 };
let projection = 'perspective';
let previewDirty = true;
let outlineEnabled = false;
let lightBrightness = 1.0;
let outlineThickness = 1.0;

// ===================================================================
//  MAIN VIEWPORT — orbit controls, free exploration
// ===================================================================
const container = $('viewport');
const mainRenderer = new THREE.WebGLRenderer({ antialias: true });
mainRenderer.setPixelRatio(window.devicePixelRatio);
mainRenderer.setClearColor(0x1e1e1e);
container.appendChild(mainRenderer.domElement);

const scene = new THREE.Scene();

const mainCamera = new THREE.PerspectiveCamera(45, 1, 0.1, 10000);
mainCamera.up.set(0, 1, 0);
mainCamera.position.set(200, 150, 200);
mainCamera.lookAt(0, 0, 0);

const orbitControls = new OrbitControls(mainCamera, mainRenderer.domElement);
orbitControls.enableDamping = true;
orbitControls.dampingFactor = 0.1;
orbitControls.target.set(0, 0, 0);
orbitControls.update();

// Lights
const light1 = new THREE.DirectionalLight(0xffffff, 1.0);
light1.position.set(0.3, 0.8, 0.5).normalize();
scene.add(light1);

const light2 = new THREE.DirectionalLight(0xbbccee, 0.6);
light2.position.set(-0.5, 0.3, -0.8).normalize();
scene.add(light2);

const ambientLight = new THREE.AmbientLight(0xffffff, 0.15);
scene.add(ambientLight);

function applyBrightness(val) {
  lightBrightness = val;
  light1.intensity = 1.0 * val;
  light2.intensity = 0.6 * val;
  // Keep ambient constant so the model stays slightly visible at brightness 0
}

// Grid
const grid = new THREE.GridHelper(200, 20, 0x30363d, 0x22272e);
grid.rotation.x = Math.PI / 2;
scene.add(grid);

// ===================================================================
//  PREVIEW CANVAS — fixed camera, auto-fit, transparent background
// ===================================================================
const previewCanvas = $('previewCanvas');
const previewOverlay = $('previewOverlay');
const previewRenderer = new THREE.WebGLRenderer({ canvas: previewCanvas, antialias: true, alpha: true });
previewRenderer.setClearColor(0x000000, 0);

const previewPerspCam = new THREE.PerspectiveCamera(45, 1, 0.1, 10000);
previewPerspCam.up.set(0, 1, 0);
const previewOrthoCam = new THREE.OrthographicCamera(-100, 100, 100, -100, 0.1, 10000);
previewOrthoCam.up.set(0, 1, 0);

function getPreviewCamera() {
  return projection === 'perspective' ? previewPerspCam : previewOrthoCam;
}

// Compute AABB from the current mesh + rotation (rotation-dependent tight fit)
function computeAABB() {
  if (!currentMesh) return;
  const pos = currentMesh.geometry.attributes.position;
  const q = currentMesh.quaternion;
  let minX = Infinity, maxX = -Infinity;
  let minY = Infinity, maxY = -Infinity;
  let minZ = Infinity, maxZ = -Infinity;
  const v = new THREE.Vector3();
  for (let i = 0; i < pos.count; i++) {
    v.fromBufferAttribute(pos, i);
    v.applyQuaternion(q);
    minX = Math.min(minX, v.x); maxX = Math.max(maxX, v.x);
    minY = Math.min(minY, v.y); maxY = Math.max(maxY, v.y);
    minZ = Math.min(minZ, v.z); maxZ = Math.max(maxZ, v.z);
  }
  aabb = {
    cx: (minX + maxX) / 2,
    cy: (minY + maxY) / 2,
    cz: (minZ + maxZ) / 2,
    halfX: Math.max((maxX - minX) / 2, 0.001),
    halfY: Math.max((maxY - minY) / 2, 0.001),
    halfZ: Math.max((maxZ - minZ) / 2, 0.001),
  };
}

function updatePreviewCamera() {
  const w = parseInt($('width').value) || 800;
  const h = parseInt($('height').value) || 600;
  const pad = Math.min(parseInt($('padding').value) || 0, Math.floor(Math.min(w, h) / 2) - 1);
  const padF = Math.max(0, pad);
  const aspect = w / h;
  const { cx, cy, cz, halfX, halfY, halfZ } = aabb;

  // Scale preview canvas to fit max 280px display size,
  // but render at full target resolution so outlines are correctly proportioned.
  const maxPx = 280;
  const scale = Math.min(maxPx / w, maxPx / h, 1);
  const pw = Math.max(1, Math.round(w * scale));
  const ph = Math.max(1, Math.round(h * scale));
  previewRenderer.setSize(w, h, false);
  previewCanvas.style.width = pw + 'px';
  previewCanvas.style.height = ph + 'px';

  if (projection === 'perspective') {
    const fov = parseFloat($('fov').value) || 45;
    const halfFovV = THREE.MathUtils.degToRad(fov) / 2;
    const halfFovH = Math.atan(aspect * Math.tan(halfFovV));
    // Shrink effective FOV by padding ratio
    const effHalfFovV = Math.atan(Math.tan(halfFovV) * Math.max(1, h - 2 * padF) / h);
    const effHalfFovH = Math.atan(Math.tan(halfFovH) * Math.max(1, w - 2 * padF) / w);
    const tanH = Math.tan(effHalfFovH);
    const tanV = Math.tan(effHalfFovV);

    // Per-vertex perspective fit
    let dist = 0.1;
    if (currentMesh) {
      const pos = currentMesh.geometry.attributes.position;
      const q = currentMesh.quaternion;
      const v = new THREE.Vector3();
      for (let i = 0; i < pos.count; i++) {
        v.fromBufferAttribute(pos, i);
        v.applyQuaternion(q);
        const dz = v.z - cz;
        dist = Math.max(dist, dz + Math.abs(v.x - cx) / tanH, dz + Math.abs(v.y - cy) / tanV);
      }
    }
    dist = Math.max(dist, halfZ + 0.1);

    const near = Math.max(0.01, (dist - halfZ) * 0.5);
    const far = (dist + halfZ) * 3;

    previewPerspCam.fov = fov;
    previewPerspCam.aspect = aspect;
    previewPerspCam.near = near;
    previewPerspCam.far = far;
    previewPerspCam.position.set(cx, cy, cz + dist);
    previewPerspCam.lookAt(cx, cy, cz);
    previewPerspCam.updateProjectionMatrix();
  } else {
    const effW = Math.max(1, w - 2 * padF);
    const effH = Math.max(1, h - 2 * padF);
    const fhhFromY = halfY * h / effH;
    const fhhFromX = halfX * w / (effW * aspect);
    const fhh = Math.max(fhhFromY, fhhFromX, 0.001);
    const fhw = fhh * aspect;

    const dist = halfZ + Math.max(fhh, fhw) * 2;
    const near = 0.01;
    const far = dist * 4;

    previewOrthoCam.left = -fhw;
    previewOrthoCam.right = fhw;
    previewOrthoCam.top = fhh;
    previewOrthoCam.bottom = -fhh;
    previewOrthoCam.near = near;
    previewOrthoCam.far = far;
    previewOrthoCam.position.set(cx, cy, cz + dist);
    previewOrthoCam.lookAt(cx, cy, cz);
    previewOrthoCam.updateProjectionMatrix();
  }
}

function renderPreview() {
  if (!currentMesh) return;
  // Hide gizmo and grid for clean preview
  const gizmoHelper = gizmo ? gizmo.getHelper() : null;
  if (gizmoHelper) gizmoHelper.visible = false;
  grid.visible = false;

  previewRenderer.render(scene, getPreviewCamera());

  grid.visible = true;
  if (gizmoHelper) gizmoHelper.visible = true;
}

// ── Gizmo management ───────────────────────
function recreateGizmo() {
  if (gizmo) {
    gizmo.detach();
    scene.remove(gizmo.getHelper());
    gizmo.dispose();
    gizmo = null;
  }
  if (!currentMesh) return;
  gizmo = new TransformControls(mainCamera, mainRenderer.domElement);
  gizmo.setMode('rotate');
  gizmo.setSize(1.2);
  gizmo.attach(currentMesh);
  scene.add(gizmo.getHelper());
  gizmo.addEventListener('objectChange', syncFieldsFromObject);
  gizmo.addEventListener('dragging-changed', e => {
    orbitControls.enabled = !e.value;
  });
}

// ── Resize ─────────────────────────────────
function resize() {
  const w = container.clientWidth, h = container.clientHeight;
  if (w === 0 || h === 0) return;
  mainCamera.aspect = w / h;
  mainCamera.updateProjectionMatrix();
  mainRenderer.setSize(w, h);
}
window.addEventListener('resize', resize);
new ResizeObserver(resize).observe(container);
resize();

// ── Render loop ────────────────────────────
function animate() {
  requestAnimationFrame(animate);
  orbitControls.update();
  mainRenderer.render(scene, mainCamera);

  if (previewDirty) {
    previewDirty = false;
    computeAABB();
    updatePreviewCamera();
    renderPreview();
  }
}
animate();

// ── Mark preview dirty ─────────────────────
function markDirty() {
  previewDirty = true;
  updateCurl();
}

// ── Sync: object rotation → input fields ──
function syncFieldsFromObject() {
  if (!currentMesh) return;
  const r = currentMesh.rotation;
  $('rotX').value = THREE.MathUtils.radToDeg(r.x).toFixed(1);
  $('rotY').value = THREE.MathUtils.radToDeg(r.y).toFixed(1);
  $('rotZ').value = THREE.MathUtils.radToDeg(r.z).toFixed(1);
  markDirty();
}

// ── Sync: input fields → object ──
function syncObjectFromFields() {
  if (!currentMesh) return;
  currentMesh.rotation.set(
    THREE.MathUtils.degToRad(parseFloat($('rotX').value) || 0),
    THREE.MathUtils.degToRad(parseFloat($('rotY').value) || 0),
    THREE.MathUtils.degToRad(parseFloat($('rotZ').value) || 0),
    'XYZ'
  );
  markDirty();
}

['rotX', 'rotY', 'rotZ'].forEach(id => $(id).addEventListener('input', syncObjectFromFields));
$('fov').addEventListener('input', markDirty);
['width', 'height', 'padding'].forEach(id => $(id).addEventListener('input', markDirty));

// ── Projection toggle ──────────────────────
$('btnPerspective').addEventListener('click', () => setProjection('perspective'));
$('btnOrthographic').addEventListener('click', () => setProjection('orthographic'));

function setProjection(proj) {
  projection = proj;
  $('btnPerspective').classList.toggle('active', proj === 'perspective');
  $('btnOrthographic').classList.toggle('active', proj === 'orthographic');
  $('fovSection').style.display = proj === 'perspective' ? '' : 'none';
  markDirty();
}

// ── Light brightness ───────────────────────
$('brightness').addEventListener('input', e => {
  const val = parseFloat(e.target.value);
  $('brightnessValue').textContent = val.toFixed(2);
  applyBrightness(val);
  markDirty();
});

// ── Outline toggle ─────────────────────────
$('outlineToggle').addEventListener('change', e => {
  outlineEnabled = e.target.checked;
  if (currentMesh) {
    if (outlineEnabled) {
      applyOutlineToModel(currentMesh);
    } else {
      removeOutlineFromModel(currentMesh);
    }
  }
  markDirty();
});

$('outlineThickness').addEventListener('input', e => {
  outlineThickness = parseFloat(e.target.value);
  $('outlineThicknessValue').textContent = outlineThickness.toFixed(1);
  if (outlineEnabled && currentMesh) {
    applyOutlineToModel(currentMesh);
  }
  markDirty();
});

function applyOutlineToModel(model) {
  removeOutlineFromModel(model);
  model.traverse((child) => {
    if (child.isMesh) {
      const mesh = child;
      const geometry = mesh.geometry;
      const edges = new THREE.EdgesGeometry(geometry);
      const outline = new THREE.LineSegments(
        edges,
        new THREE.LineBasicMaterial({ color: 0x000000, linewidth: outlineThickness }),
      );
      outline.userData.isOutline = true;
      mesh.add(outline);
    }
  });
}

function removeOutlineFromModel(model) {
  const toRemove = [];
  model.traverse((child) => {
    if (child.isLineSegments && child.userData.isOutline) {
      toRemove.push(child);
    }
  });
  toRemove.forEach(c => {
    c.parent.remove(c);
    c.geometry.dispose();
    c.material.dispose();
  });
}

// ── Color picker ───────────────────────────
$('colorPicker').addEventListener('input', e => {
  const hex = e.target.value;
  $('colorHex').textContent = hex;
  if (currentMesh) currentMesh.material.color.set(hex);
  markDirty();
});

// ── Reset ──────────────────────────────────
$('resetBtn').addEventListener('click', () => {
  $('rotX').value = '0';
  $('rotY').value = '0';
  $('rotZ').value = '0';
  $('fov').value = '45';
  $('padding').value = '10';
  $('colorPicker').value = '#8ca0c8';
  $('colorHex').textContent = '#8ca0c8';
  $('outlineToggle').checked = false;
  outlineEnabled = false;
  $('outlineThickness').value = '1';
  $('outlineThicknessValue').textContent = '1.0';
  outlineThickness = 1.0;
  $('brightness').value = '1';
  $('brightnessValue').textContent = '1.00';
  applyBrightness(1.0);
  setProjection('perspective');
  if (currentMesh) {
    removeOutlineFromModel(currentMesh);
    currentMesh.rotation.set(0, 0, 0);
    currentMesh.material.color.set('#8ca0c8');
  }
  mainCamera.position.set(200, 150, 200);
  mainCamera.lookAt(0, 0, 0);
  orbitControls.target.set(0, 0, 0);
  orbitControls.update();
  markDirty();
});

// ── File handling ──────────────────────────
const dropZone = $('dropZone');
const fileInput = $('fileInput');

dropZone.addEventListener('click', () => fileInput.click());
dropZone.addEventListener('dragover', e => { e.preventDefault(); dropZone.classList.add('dragover'); });
dropZone.addEventListener('dragleave', () => dropZone.classList.remove('dragover'));
dropZone.addEventListener('drop', e => {
  e.preventDefault();
  dropZone.classList.remove('dragover');
  if (e.dataTransfer.files.length) loadFile(e.dataTransfer.files[0]);
});
fileInput.addEventListener('change', () => { if (fileInput.files.length) loadFile(fileInput.files[0]); });

function loadFile(f) {
  modelFile = f;
  $('fileName').textContent = f.name + ' (' + (f.size / 1024).toFixed(0) + ' KB)';
  $('renderBtn').disabled = false;
  const reader = new FileReader();
  reader.onload = ev => loadIntoScene(ev.target.result, f.name);
  reader.readAsArrayBuffer(f);
}

function loadIntoScene(buffer, fileName) {
  if (gizmo) { gizmo.detach(); scene.remove(gizmo.getHelper()); gizmo.dispose(); gizmo = null; }
  if (currentMesh) { scene.remove(currentMesh); currentMesh.geometry.dispose(); currentMesh.material.dispose(); }

  const ext = (fileName || '').split('.').pop().toLowerCase();

  if (ext === 'glb' || ext === 'gltf') {
    const loader = new GLTFLoader();
    const blob = new Blob([buffer]);
    const url = URL.createObjectURL(blob);
    loader.load(url, (gltfResult) => {
      URL.revokeObjectURL(url);
      // Merge all meshes into one geometry
      const merged = new THREE.BufferGeometry();
      const positions = [];
      const normals = [];
      gltfResult.scene.updateMatrixWorld(true);
      gltfResult.scene.traverse((child) => {
        if (child.isMesh) {
          const geom = child.geometry.clone();
          geom.applyMatrix4(child.matrixWorld);
          const pos = geom.attributes.position;
          const norm = geom.attributes.normal;
          if (geom.index) {
            const idx = geom.index;
            for (let i = 0; i < idx.count; i++) {
              const vi = idx.getX(i);
              positions.push(pos.getX(vi), pos.getY(vi), pos.getZ(vi));
              if (norm) normals.push(norm.getX(vi), norm.getY(vi), norm.getZ(vi));
              else normals.push(0, 0, 0);
            }
          } else {
            for (let i = 0; i < pos.count; i++) {
              positions.push(pos.getX(i), pos.getY(i), pos.getZ(i));
              if (norm) normals.push(norm.getX(i), norm.getY(i), norm.getZ(i));
              else normals.push(0, 0, 0);
            }
          }
          geom.dispose();
        }
      });
      if (positions.length === 0) return;
      merged.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
      merged.setAttribute('normal', new THREE.Float32BufferAttribute(normals, 3));
      merged.computeVertexNormals();
      finishLoad(merged);
    }, undefined, (err) => {
      console.error('GLB load error', err);
    });
  } else if (ext === 'obj') {
    const text = new TextDecoder().decode(buffer);
    const loader = new OBJLoader();
    const group = loader.parse(text);
    // Merge all meshes
    const merged = new THREE.BufferGeometry();
    const positions = [];
    const normals = [];
    group.traverse((child) => {
      if (child.isMesh) {
        const geom = child.geometry;
        const pos = geom.attributes.position;
        const norm = geom.attributes.normal;
        if (geom.index) {
          const idx = geom.index;
          for (let i = 0; i < idx.count; i++) {
            const vi = idx.getX(i);
            positions.push(pos.getX(vi), pos.getY(vi), pos.getZ(vi));
            if (norm) normals.push(norm.getX(vi), norm.getY(vi), norm.getZ(vi));
            else normals.push(0, 0, 0);
          }
        } else {
          for (let i = 0; i < pos.count; i++) {
            positions.push(pos.getX(i), pos.getY(i), pos.getZ(i));
            if (norm) normals.push(norm.getX(i), norm.getY(i), norm.getZ(i));
            else normals.push(0, 0, 0);
          }
        }
      }
    });
    if (positions.length === 0) return;
    merged.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
    merged.setAttribute('normal', new THREE.Float32BufferAttribute(normals, 3));
    merged.computeVertexNormals();
    finishLoad(merged);
  } else if (ext === 'tjs' || ext === 'json') {
    // CadQuery Three.js JSON export (legacy format v3)
    const text = new TextDecoder().decode(buffer);
    const tjs = JSON.parse(text);
    const geometry = parseTjsGeometry(tjs);
    if (!geometry) return;
    finishLoad(geometry);
  } else {
    // STL (default)
    const geometry = new STLLoader().parse(buffer);
    geometry.computeVertexNormals();
    finishLoad(geometry);
  }
}

// ── TJS (CadQuery Three.js JSON) parser ─────────────────────────────────────
function parseTjsGeometry(tjs) {
  const positions = [];
  const normals = [];
  const uvs = [];
  const scale = tjs.scale || 1.0;

  const FACE_TYPE_MASK = 0x07;
  const IS_QUAD = 1;

  let offset = 0;
  while (offset < tjs.faces.length) {
    const type = tjs.faces[offset++];

    const isQuad = (type & FACE_TYPE_MASK) === IS_QUAD;
    const hasMaterial = (type & 0x08) !== 0;
    const hasUV = (type & 0x10) !== 0;
    const hasNormal = (type & 0x20) !== 0;
    const hasColor = (type & 0x40) !== 0;

    let faceVerts = [];
    if (isQuad) {
      faceVerts = [
        tjs.faces[offset++], tjs.faces[offset++],
        tjs.faces[offset++], tjs.faces[offset++],
      ];
      [[0, 1, 2], [0, 2, 3]].forEach((tri) => {
        tri.forEach((vi) => {
          const idx = faceVerts[vi];
          positions.push(
            tjs.vertices[idx * 3 + 0] * scale,
            tjs.vertices[idx * 3 + 1] * scale,
            tjs.vertices[idx * 3 + 2] * scale,
          );
          if (tjs.normals.length > 0 && hasNormal) {
            normals.push(tjs.normals[idx * 3], tjs.normals[idx * 3 + 1], tjs.normals[idx * 3 + 2]);
          }
          if (tjs.uvs.length > 0 && tjs.uvs[0].length > 0 && hasUV) {
            uvs.push(tjs.uvs[0][idx * 2], tjs.uvs[0][idx * 2 + 1]);
          }
        });
      });
    } else {
      faceVerts = [tjs.faces[offset++], tjs.faces[offset++], tjs.faces[offset++]];
      [0, 1, 2].forEach((vi) => {
        const idx = faceVerts[vi];
        positions.push(
          tjs.vertices[idx * 3 + 0] * scale,
          tjs.vertices[idx * 3 + 1] * scale,
          tjs.vertices[idx * 3 + 2] * scale,
        );
        if (tjs.normals.length > 0 && hasNormal) {
          normals.push(tjs.normals[idx * 3], tjs.normals[idx * 3 + 1], tjs.normals[idx * 3 + 2]);
        }
        if (tjs.uvs.length > 0 && tjs.uvs[0].length > 0 && hasUV) {
          uvs.push(tjs.uvs[0][idx * 2], tjs.uvs[0][idx * 2 + 1]);
        }
      });
    }

    // Skip optional trailing data
    if (hasMaterial) offset++;
    if (hasUV) offset += isQuad ? 4 : 3;
    if (hasNormal) offset += isQuad ? 4 : 3;
    if (hasColor) offset += isQuad ? 4 : 3;
  }

  if (positions.length === 0) return null;

  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
  if (normals.length > 0) {
    geometry.setAttribute('normal', new THREE.Float32BufferAttribute(normals, 3));
  } else {
    geometry.computeVertexNormals();
  }
  if (uvs.length > 0) {
    geometry.setAttribute('uv', new THREE.Float32BufferAttribute(uvs, 2));
  }
  return geometry;
}

function finishLoad(geometry) {

  // Center at origin
  geometry.computeBoundingBox();
  const center = new THREE.Vector3();
  geometry.boundingBox.getCenter(center);
  geometry.translate(-center.x, -center.y, -center.z);
  geometry.computeBoundingBox();

  const hexColor = $('colorPicker').value;
  currentMesh = new THREE.Mesh(geometry, new THREE.MeshPhongMaterial({
    color: hexColor,
    specular: 0x222222,
    shininess: 30,
    flatShading: true,
  }));
  scene.add(currentMesh);

  // Apply outline if enabled
  if (outlineEnabled) {
    applyOutlineToModel(currentMesh);
  }

  // Compute size
  const size = new THREE.Vector3();
  geometry.boundingBox.getSize(size);
  const maxDim = Math.max(size.x, size.y, size.z);

  // Set near/far for all cameras
  const near = maxDim * 0.01;
  const far = maxDim * 10;
  mainCamera.near = near;
  mainCamera.far = far;
  mainCamera.updateProjectionMatrix();

  // Fit main camera nicely
  const d = maxDim * 1.5;
  mainCamera.position.set(d * 0.7, d * 0.5, d * 0.7);
  mainCamera.lookAt(0, 0, 0);
  orbitControls.target.set(0, 0, 0);
  orbitControls.update();

  recreateGizmo();

  $('viewportHint').style.display = 'none';
  previewOverlay.classList.add('visible');
  grid.scale.setScalar(maxDim / 200);

  syncObjectFromFields();
  markDirty();
}

// ── Curl preview ───────────────────────────
function updateCurl() {
  if (!modelFile) {
    $('curlPreview').textContent = 'Upload a file to see the curl command';
    return;
  }
  const p = new URLSearchParams({
    width:      $('width').value,
    height:     $('height').value,
    rot_x:      $('rotX').value,
    rot_y:      $('rotY').value,
    rot_z:      $('rotZ').value,
    fov:        $('fov').value,
    projection: projection,
    color:      $('colorPicker').value.replace('#', ''),
    padding:    $('padding').value,
    outline:    outlineEnabled,
    brightness: parseFloat($('brightness').value),
    outline_thickness: outlineThickness,
  });
  $('curlPreview').textContent =
    `curl -X POST "${location.origin}/render?${p}" \\\n  -F "file=@${modelFile.name}" \\\n  -o render.png`;
}

// ── Render PNG ─────────────────────────────
$('renderBtn').addEventListener('click', doRender);
let lastBlobUrl = null;

async function doRender() {
  if (!modelFile) return;
  $('renderBtn').disabled = true;
  $('statusMsg').textContent = 'Rendering…';

  const params = new URLSearchParams({
    width:      $('width').value,
    height:     $('height').value,
    rot_x:      $('rotX').value,
    rot_y:      $('rotY').value,
    rot_z:      $('rotZ').value,
    fov:        $('fov').value,
    projection: projection,
    color:      $('colorPicker').value.replace('#', ''),
    padding:    $('padding').value,
    outline:    outlineEnabled,
    brightness: parseFloat($('brightness').value),
    outline_thickness: outlineThickness,
  });

  const form = new FormData();
  form.append('file', modelFile);

  const t0 = performance.now();
  try {
    const res = await fetch('/render?' + params, { method: 'POST', body: form });
    const elapsed = ((performance.now() - t0) / 1000).toFixed(2);

    if (!res.ok) {
      const err = await res.json().catch(() => ({ error: res.statusText }));
      $('statusMsg').textContent = 'Error: ' + (err.error || res.statusText);
      return;
    }

    const blob = await res.blob();
    lastBlobUrl && URL.revokeObjectURL(lastBlobUrl);
    lastBlobUrl = URL.createObjectURL(blob);

    $('lightboxImg').src = lastBlobUrl;
    $('lightbox').classList.add('active');

    const dlBtn = $('dlBtn');
    dlBtn.style.display = 'block';
    dlBtn.onclick = () => {
      const a = document.createElement('a');
      a.href = lastBlobUrl;
      a.download = 'render.png';
      a.click();
    };

    $('statusMsg').textContent =
      `${$('width').value}×${$('height').value} · ${(blob.size/1024).toFixed(0)} KB · ${elapsed}s`;
  } catch (e) {
    $('statusMsg').textContent = 'Network error: ' + e.message;
  } finally {
    $('renderBtn').disabled = false;
  }
}

// ── Lightbox ───────────────────────────────
$('lightbox').addEventListener('click', e => {
  if (e.target.tagName !== 'IMG') $('lightbox').classList.remove('active');
});
