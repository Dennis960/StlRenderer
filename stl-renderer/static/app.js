import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import { TransformControls } from 'three/addons/controls/TransformControls.js';
import { STLLoader } from 'three/addons/loaders/STLLoader.js';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import { OBJLoader } from 'three/addons/loaders/OBJLoader.js';

const $ = id => document.getElementById(id);

// ── State ──────────────────────────────────
/** All File objects currently loaded (one per uploaded model). */
let modelFiles = [];
/** Individual THREE.Mesh objects, one per uploaded model. */
let loadedMeshes = [];
/** The THREE.Group that contains all loadedMeshes. Rotation is applied to this. */
let currentGroup = null;
let gizmo = null;
let aabb = { cx: 0, cy: 0, cz: 0, halfX: 1, halfY: 1, halfZ: 1 };
let projection = 'perspective';
let previewDirty = true;
let outlineEnabled = false;
let lightBrightness = 1.0;
let outlineThickness = 1.0;

/** Default color palette for multi-model uploads. */
const DEFAULT_COLORS = ['#8ca0c8', '#c88a8a', '#8ac88a', '#c8c88a', '#8ac8c8', '#c88ac8'];

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

// Compute AABB from all loaded meshes + the group's rotation (tight fit)
function computeAABB() {
  if (!currentGroup || loadedMeshes.length === 0) return;
  const q = currentGroup.quaternion;
  let minX = Infinity, maxX = -Infinity;
  let minY = Infinity, maxY = -Infinity;
  let minZ = Infinity, maxZ = -Infinity;
  const v = new THREE.Vector3();
  for (const mesh of loadedMeshes) {
    const pos = mesh.geometry.attributes.position;
    for (let i = 0; i < pos.count; i++) {
      v.fromBufferAttribute(pos, i);
      v.applyQuaternion(q);
      minX = Math.min(minX, v.x); maxX = Math.max(maxX, v.x);
      minY = Math.min(minY, v.y); maxY = Math.max(maxY, v.y);
      minZ = Math.min(minZ, v.z); maxZ = Math.max(maxZ, v.z);
    }
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

    // Per-vertex perspective fit across all loaded meshes
    let dist = 0.1;
    if (currentGroup && loadedMeshes.length > 0) {
      const q = currentGroup.quaternion;
      const v = new THREE.Vector3();
      for (const mesh of loadedMeshes) {
        const pos = mesh.geometry.attributes.position;
        for (let i = 0; i < pos.count; i++) {
          v.fromBufferAttribute(pos, i);
          v.applyQuaternion(q);
          const dz = v.z - cz;
          dist = Math.max(dist, dz + Math.abs(v.x - cx) / tanH, dz + Math.abs(v.y - cy) / tanV);
        }
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
  if (!currentGroup) return;
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
  if (!currentGroup) return;
  gizmo = new TransformControls(mainCamera, mainRenderer.domElement);
  gizmo.setMode('rotate');
  gizmo.setSize(1.2);
  gizmo.attach(currentGroup);
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
  if (!currentGroup) return;
  const r = currentGroup.rotation;
  $('rotX').value = THREE.MathUtils.radToDeg(r.x).toFixed(1);
  $('rotY').value = THREE.MathUtils.radToDeg(r.y).toFixed(1);
  $('rotZ').value = THREE.MathUtils.radToDeg(r.z).toFixed(1);
  markDirty();
}

// ── Sync: input fields → object ──
function syncObjectFromFields() {
  if (!currentGroup) return;
  currentGroup.rotation.set(
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
  if (currentGroup) {
    if (outlineEnabled) {
      applyOutlineToModel(currentGroup);
    } else {
      removeOutlineFromModel(currentGroup);
    }
  }
  markDirty();
});

$('outlineThickness').addEventListener('input', e => {
  outlineThickness = parseFloat(e.target.value);
  $('outlineThicknessValue').textContent = outlineThickness.toFixed(1);
  if (outlineEnabled && currentGroup) {
    applyOutlineToModel(currentGroup);
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

// ── Reset ──────────────────────────────────
$('resetBtn').addEventListener('click', () => {
  $('rotX').value = '0';
  $('rotY').value = '0';
  $('rotZ').value = '0';
  $('fov').value = '45';
  $('padding').value = '10';
  $('outlineToggle').checked = false;
  outlineEnabled = false;
  $('outlineThickness').value = '1';
  $('outlineThicknessValue').textContent = '1.0';
  outlineThickness = 1.0;
  $('brightness').value = '1';
  $('brightnessValue').textContent = '1.00';
  applyBrightness(1.0);
  setProjection('perspective');
  if (currentGroup) {
    removeOutlineFromModel(currentGroup);
    currentGroup.rotation.set(0, 0, 0);
    // Reset each mesh's colour to its default palette entry
    loadedMeshes.forEach((mesh, i) => {
      const col = DEFAULT_COLORS[i % DEFAULT_COLORS.length];
      mesh.material.color.set(col);
    });
    renderModelColorsUI();
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
  if (e.dataTransfer.files.length) loadFiles(e.dataTransfer.files);
});
fileInput.addEventListener('change', () => { if (fileInput.files.length) loadFiles(fileInput.files); });

/** Load one or more files, preserving their relative 3D positions. */
async function loadFiles(fileList) {
  const files = Array.from(fileList);
  if (files.length === 0) return;

  // Clear existing scene objects
  if (gizmo) { gizmo.detach(); scene.remove(gizmo.getHelper()); gizmo.dispose(); gizmo = null; }
  if (currentGroup) {
    scene.remove(currentGroup);
    loadedMeshes.forEach(m => { m.geometry.dispose(); m.material.dispose(); });
  }
  loadedMeshes = [];
  currentGroup = null;

  // Parse all files in parallel, keeping original coordinates (no per-file centering)
  const geometries = await Promise.all(files.map(f =>
    new Promise(resolve => {
      const reader = new FileReader();
      reader.onload = ev => resolve(parseFileToGeometry(ev.target.result, f.name));
      reader.readAsArrayBuffer(f);
    })
  ));

  // Keep only successfully parsed models
  const validPairs = files
    .map((f, i) => [f, geometries[i]])
    .filter(([, g]) => g !== null);
  if (validPairs.length === 0) return;

  setupInScene(validPairs.map(([f]) => f), validPairs.map(([, g]) => g));
}

/**
 * Parse a file buffer into a BufferGeometry WITHOUT centering.
 * Returns a Promise that resolves to the geometry (or null on error).
 */
function parseFileToGeometry(buffer, fileName) {
  const ext = (fileName || '').split('.').pop().toLowerCase();

  if (ext === 'glb' || ext === 'gltf') {
    return new Promise(resolve => {
      const loader = new GLTFLoader();
      const blob = new Blob([buffer]);
      const url = URL.createObjectURL(blob);
      loader.load(url, gltfResult => {
        URL.revokeObjectURL(url);
        const positions = [], normals = [];
        gltfResult.scene.updateMatrixWorld(true);
        gltfResult.scene.traverse(child => {
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
                normals.push(norm ? norm.getX(vi) : 0, norm ? norm.getY(vi) : 0, norm ? norm.getZ(vi) : 0);
              }
            } else {
              for (let i = 0; i < pos.count; i++) {
                positions.push(pos.getX(i), pos.getY(i), pos.getZ(i));
                normals.push(norm ? norm.getX(i) : 0, norm ? norm.getY(i) : 0, norm ? norm.getZ(i) : 0);
              }
            }
            geom.dispose();
          }
        });
        if (positions.length === 0) { resolve(null); return; }
        const merged = new THREE.BufferGeometry();
        merged.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
        merged.setAttribute('normal', new THREE.Float32BufferAttribute(normals, 3));
        merged.computeVertexNormals();
        resolve(merged);
      }, undefined, () => resolve(null));
    });
  }

  if (ext === 'obj') {
    const text = new TextDecoder().decode(buffer);
    const group = new OBJLoader().parse(text);
    const positions = [], normals = [];
    group.traverse(child => {
      if (child.isMesh) {
        const geom = child.geometry;
        const pos = geom.attributes.position;
        const norm = geom.attributes.normal;
        if (geom.index) {
          const idx = geom.index;
          for (let i = 0; i < idx.count; i++) {
            const vi = idx.getX(i);
            positions.push(pos.getX(vi), pos.getY(vi), pos.getZ(vi));
            normals.push(norm ? norm.getX(vi) : 0, norm ? norm.getY(vi) : 0, norm ? norm.getZ(vi) : 0);
          }
        } else {
          for (let i = 0; i < pos.count; i++) {
            positions.push(pos.getX(i), pos.getY(i), pos.getZ(i));
            normals.push(norm ? norm.getX(i) : 0, norm ? norm.getY(i) : 0, norm ? norm.getZ(i) : 0);
          }
        }
      }
    });
    if (positions.length === 0) return Promise.resolve(null);
    const merged = new THREE.BufferGeometry();
    merged.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
    merged.setAttribute('normal', new THREE.Float32BufferAttribute(normals, 3));
    merged.computeVertexNormals();
    return Promise.resolve(merged);
  }

  if (ext === 'tjs' || ext === 'json') {
    const tjs = JSON.parse(new TextDecoder().decode(buffer));
    return Promise.resolve(parseTjsGeometry(tjs));
  }

  // Default: STL
  const geometry = new STLLoader().parse(buffer);
  geometry.computeVertexNormals();
  return Promise.resolve(geometry);
}

/**
 * Place all parsed geometries in the scene as a single group.
 * All geometries are shifted by the combined bounding-box centre so their
 * relative positions are preserved while the whole group is at the origin.
 */
function setupInScene(files, geometries) {
  // Compute combined bounding box (geometries still at their original positions)
  let minX = Infinity, maxX = -Infinity;
  let minY = Infinity, maxY = -Infinity;
  let minZ = Infinity, maxZ = -Infinity;
  geometries.forEach(geom => {
    geom.computeBoundingBox();
    const b = geom.boundingBox;
    minX = Math.min(minX, b.min.x); maxX = Math.max(maxX, b.max.x);
    minY = Math.min(minY, b.min.y); maxY = Math.max(maxY, b.max.y);
    minZ = Math.min(minZ, b.min.z); maxZ = Math.max(maxZ, b.max.z);
  });

  const cx = (minX + maxX) / 2;
  const cy = (minY + maxY) / 2;
  const cz = (minZ + maxZ) / 2;

  // Shift all geometries so the combined center sits at the origin
  geometries.forEach(geom => geom.translate(-cx, -cy, -cz));

  currentGroup = new THREE.Group();
  modelFiles = files;
  loadedMeshes = [];

  geometries.forEach((geom, i) => {
    const color = DEFAULT_COLORS[i % DEFAULT_COLORS.length];
    const mesh = new THREE.Mesh(geom, new THREE.MeshPhongMaterial({
      color,
      specular: 0x222222,
      shininess: 30,
      flatShading: true,
    }));
    currentGroup.add(mesh);
    loadedMeshes.push(mesh);
  });

  scene.add(currentGroup);

  // Apply outline if enabled
  if (outlineEnabled) applyOutlineToModel(currentGroup);

  // Fit main camera
  const size = new THREE.Vector3(maxX - minX, maxY - minY, maxZ - minZ);
  const maxDim = Math.max(size.x, size.y, size.z);
  const near = maxDim * 0.01;
  const far = maxDim * 10;
  mainCamera.near = near;
  mainCamera.far = far;
  mainCamera.updateProjectionMatrix();

  const d = maxDim * 1.5;
  mainCamera.position.set(d * 0.7, d * 0.5, d * 0.7);
  mainCamera.lookAt(0, 0, 0);
  orbitControls.target.set(0, 0, 0);
  orbitControls.update();
  grid.scale.setScalar(maxDim / 200);

  // Update filename display
  if (files.length === 1) {
    $('fileName').textContent = files[0].name + ' (' + (files[0].size / 1024).toFixed(0) + ' KB)';
  } else {
    const totalKB = files.reduce((s, f) => s + f.size, 0) / 1024;
    $('fileName').textContent = files.length + ' files (' + totalKB.toFixed(0) + ' KB total)';
  }

  $('renderBtn').disabled = false;
  $('viewportHint').style.display = 'none';
  previewOverlay.classList.add('visible');

  renderModelColorsUI();
  recreateGizmo();
  syncObjectFromFields();
  markDirty();
}

/** Render the per-model colour pickers into #modelColorsList. */
function renderModelColorsUI() {
  const container = $('modelColorsList');
  container.innerHTML = '';
  loadedMeshes.forEach((mesh, i) => {
    const hexColor = '#' + mesh.material.color.getHexString();
    const name = modelFiles[i] ? modelFiles[i].name : ('Model ' + (i + 1));
    const row = document.createElement('div');
    row.className = 'color-row';

    const picker = document.createElement('input');
    picker.type = 'color';
    picker.value = hexColor;
    picker.addEventListener('input', e => {
      mesh.material.color.set(e.target.value);
      hexSpan.textContent = e.target.value;
      markDirty();
    });

    const hexSpan = document.createElement('span');
    hexSpan.className = 'color-hex';
    hexSpan.textContent = hexColor;

    const nameSpan = document.createElement('span');
    nameSpan.className = 'model-color-name';
    nameSpan.title = name;
    nameSpan.textContent = name;

    row.appendChild(picker);
    row.appendChild(hexSpan);
    if (loadedMeshes.length > 1) row.appendChild(nameSpan);
    container.appendChild(row);
  });

  // Fallback: if nothing loaded yet, keep a placeholder picker
  if (loadedMeshes.length === 0) {
    const row = document.createElement('div');
    row.className = 'color-row';
    const picker = document.createElement('input');
    picker.type = 'color';
    picker.id = 'colorPicker';
    picker.value = '#8ca0c8';
    const hexSpan = document.createElement('span');
    hexSpan.className = 'color-hex';
    hexSpan.id = 'colorHex';
    hexSpan.textContent = '#8ca0c8';
    row.appendChild(picker);
    row.appendChild(hexSpan);
    container.appendChild(row);
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


// ── Curl preview ───────────────────────────
function updateCurl() {
  if (modelFiles.length === 0) {
    $('curlPreview').textContent = 'Upload a file to see the curl command';
    return;
  }
  const colors = loadedMeshes.map(m => m.material.color.getHexString()).join(',');
  const p = new URLSearchParams({
    width:             $('width').value,
    height:            $('height').value,
    rot_x:             $('rotX').value,
    rot_y:             $('rotY').value,
    rot_z:             $('rotZ').value,
    fov:               $('fov').value,
    projection:        projection,
    colors,
    padding:           $('padding').value,
    outline:           outlineEnabled,
    brightness:        parseFloat($('brightness').value),
    outline_thickness: outlineThickness,
  });
  const fileArgs = modelFiles.map(f => `-F "file=@${f.name}"`).join(' \\\n  ');
  $('curlPreview').textContent =
    `curl -X POST "${location.origin}/render?${p}" \\\n  ${fileArgs} \\\n  -o render.png`;
}

// ── Render PNG ─────────────────────────────
$('renderBtn').addEventListener('click', doRender);
let lastBlobUrl = null;

async function doRender() {
  if (modelFiles.length === 0) return;
  $('renderBtn').disabled = true;
  $('statusMsg').textContent = 'Rendering…';

  const colors = loadedMeshes.map(m => m.material.color.getHexString()).join(',');
  const params = new URLSearchParams({
    width:             $('width').value,
    height:            $('height').value,
    rot_x:             $('rotX').value,
    rot_y:             $('rotY').value,
    rot_z:             $('rotZ').value,
    fov:               $('fov').value,
    projection:        projection,
    colors,
    padding:           $('padding').value,
    outline:           outlineEnabled,
    brightness:        parseFloat($('brightness').value),
    outline_thickness: outlineThickness,
  });

  const form = new FormData();
  modelFiles.forEach(f => form.append('file', f));

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
