# 3D Model Renderer

Dockerised microservice that renders 3D model files to PNG images using a Rust software rasterizer. Includes a Three.js live-preview frontend.

**Supported formats:** STL (binary & ASCII), GLB/glTF, OBJ, TJS (CadQuery Three.js JSON)

Live demo at [https://stlrenderer.dennis960.com](https://stlrenderer.dennis960.com).

**There is no rate limiting. If someone abuses the public API, I will shut down the demo server, so please be considerate.**

![Screenshot](Screenshot.png)

## Quick Start

### Build from source:

```bash
docker compose up --build
# Open http://localhost:8080
```

Or with plain Docker:

```bash
docker build -t stl-renderer .
docker run -p 8080:8080 stl-renderer
```

### Pre-built image:

Use the pre-built docker image:

```bash
docker run -p 8080:8080 ghcr.io/dennis960/stlrenderer:latest
```

Or the example [docker-compose.yml](docker-compose.yml):

## API

### `GET /health`

Returns `{"status":"ok"}`.

### `POST /render`

Upload a 3D model (STL, GLB, glTF, OBJ, or TJS) as multipart form data, receive a PNG.

**Query parameters:**

| Param        | Type   | Default       | Description                                                 |
| ------------ | ------ | ------------- | ----------------------------------------------------------- |
| `width`      | int    | 800           | Image width (1–4096)                                        |
| `height`     | int    | 600           | Image height (1–4096)                                       |
| `rot_x`      | float  | 0             | Rotate model around X axis (degrees)                        |
| `rot_y`      | float  | 0             | Rotate model around Y axis (degrees)                        |
| `rot_z`      | float  | 0             | Rotate model around Z axis (degrees)                        |
| `fov`        | float  | 45            | Camera field of view (degrees, only for perspective camera) |
| `projection` | string | "perspective" | Projection type: "perspective" or "orthographic"            |
| `color`      | string | "#cccccc"     | Model color (hex code)                                      |
| `padding`    | int    | 10            | Additional padding around the model (pixels)                |
| `outline`    | bool   | false         | Draw black edge outlines on the model geometry              |

**Example with curl:**

```bash
# STL
curl -X POST "http://localhost:8080/render?width=1024&height=768&rot_x=30&rot_y=45&fov=60" \
  -F "file=@model.stl" -o render.png

# GLB
curl -X POST "http://localhost:8080/render?width=800&height=600" \
  -F "file=@model.glb" -o render.png

# OBJ
curl -X POST "http://localhost:8080/render?width=800&height=600&color=cc8844" \
  -F "file=@model.obj" -o render.png

# TJS (CadQuery Three.js JSON)
curl -X POST "http://localhost:8080/render?width=800&height=600" \
  -F "file=@model.json" -o render.png

# With outline
curl -X POST "http://localhost:8080/render?width=800&height=600&outline=true" \
  -F "file=@model.stl" -o render.png
```
