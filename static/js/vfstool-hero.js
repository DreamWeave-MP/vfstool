// vfstool's hero: OpenMW's virtual file system as the stack it is, rendered live with three.js.
//
// The sources of the example install on the docs' Start here page stand in load order, lowest
// priority at the bottom, numbered as the reports number them: Morrowind.bsa and Tribunal.bsa as
// sealed archive vaults, a membrane above them because every loose file outranks every archive,
// then the data directories as glass trays, Data Files to data-local. Each file is a tile in its
// key's column on the tray of every source that has it. The highest copy is the winner and burns;
// the copies it overrides sit ghosted beneath it, in its shadow, cast by a light straight above.
// Old Wood's only file is overridden, so the whole tray is shadowed, as `vfstool shadowed` says.
//
// Lookups fall through the stack as beams and stop at the winner. Every so often the stack runs
// `collapse`: the trays close into one folder that holds only the winners, then open again. The
// pointer is a lamp the stack leans towards, and a tile under it rises a little.
//
// The scene renders to a half-float target with shadow maps; a bright pass and four blur passes
// make the bloom, and the composite applies ACES tone mapping, a scrim behind the text, and
// dithering. Colours come from the site's CSS tokens. The stack stands beside the hero's text, or
// above it on a phone, where sass/brand.sass leaves room. Nothing runs off screen or in a hidden
// tab, the resolution drops if frames run slow, and under prefers-reduced-motion one frame is
// drawn. Until the first frame, and without WebGL, a still of the stack stands in its place.

import * as THREE from './vendor/three.module.min.js';

// The example install, lowest priority first: its source_index order.
const SOURCES = [
  { name: 'Morrowind.bsa', kind: 'archive' },
  { name: 'Tribunal.bsa', kind: 'archive' },
  { name: 'Data Files', kind: 'loose_dir' },
  { name: 'Old Wood', kind: 'loose_dir' },
  { name: 'Lantern Glow', kind: 'loose_dir' },
  { name: 'Crisp Textures', kind: 'loose_dir' },
  { name: 'data-local', kind: 'loose_dir' },
];
const ARCHIVES = 2;

// Tile faces in the icon atlas, four by four.
const FACE = {
  wood: 0, stone: 1, moss: 2, mesh: 3, glow: 4, sound: 5, lua: 6, plugin: 7,
  archive: 8, rock: 9, planks: 10, gold: 11, book: 12, music: 13, font: 14, lamp: 15,
};

// Keys on a seven by three grid, from the back row; `from` lists each key's providers by source
// index. A phone shows the first four columns.
const KEYS = [
  { col: 0, row: 0, face: FACE.mesh, key: 'meshes/xbase_anim.nif', from: [0] },
  { col: 1, row: 0, face: FACE.stone, key: 'textures/tx_stone_01.dds', from: [0, 2] },
  { col: 2, row: 0, face: FACE.music, key: 'music/explore/mx_explore_1.mp3', from: [0] },
  { col: 3, row: 0, face: FACE.gold, key: 'textures/tx_tribunal_throne.dds', from: [1] },
  { col: 4, row: 0, face: FACE.font, key: 'fonts/magic_cards.fnt', from: [0] },
  { col: 5, row: 0, face: FACE.lamp, key: 'meshes/l/light_com_lantern_01.nif', from: [0, 4] },
  { col: 6, row: 0, face: FACE.book, key: 'icons/m/tx_book.dds', from: [0, 5] },
  { col: 0, row: 1, face: FACE.moss, key: 'textures/tx_bc_moss.dds', from: [0, 2, 5] },
  { col: 1, row: 1, face: FACE.mesh, key: 'meshes/x/ex_hlaalu_b_01.nif', from: [0, 1] },
  { col: 2, row: 1, face: FACE.wood, key: 'textures/tx_wood_01.dds', from: [0, 1, 3, 4, 5] },
  { col: 3, row: 1, face: FACE.planks, key: 'textures/tx_bridge.dds', from: [1, 5] },
  { col: 4, row: 1, face: FACE.glow, key: 'textures/tx_lantern_glow.dds', from: [4] },
  { col: 5, row: 1, face: FACE.sound, key: 'sound/fx/torch.wav', from: [0, 4] },
  { col: 6, row: 1, face: FACE.rock, key: 'textures/tx_crisp_rock.dds', from: [5] },
  { col: 0, row: 2, face: FACE.archive, key: 'morrowind.bsa', from: [2] },
  { col: 1, row: 2, face: FACE.plugin, key: 'morrowind.esm', from: [2] },
  { col: 2, row: 2, face: FACE.lua, key: 'scripts/lanternglow/player.lua', from: [4] },
  { col: 3, row: 2, face: FACE.plugin, key: 'lantern glow.esp', from: [4] },
  { col: 4, row: 2, face: FACE.archive, key: 'tribunal.bsa', from: [2] },
  { col: 5, row: 2, face: FACE.plugin, key: 'tribunal.esm', from: [2] },
  { col: 6, row: 2, face: FACE.moss, key: 'textures/tx_ashland_grass.dds', from: [0, 2, 4] },
];

// The key `vfstool explain` is asked about on the Start here page, and what each provider calls it.
const EXPLAIN_KEY = 'textures/tx_wood_01.dds';
const EXPLAIN_QUERY = "vfstool explain 'Textures\\TX_WOOD_01.dds'";
const SPELLING = { 0: 'textures\\tx_wood_01.dds', 1: 'textures\\tx_wood_01.dds', 3: 'Textures/Tx_Wood_01.dds', 4: 'Textures/Tx_Wood_01.dds', 5: 'Textures/Tx_Wood_01.dds' };

const CELL = 0.46;
// Where `explain` lines the providers up: in front of the stack, facing the viewer.
const LADDER = { x: -1.3, labelX: 0.3, z: 1.9, step: 0.4, lift: -0.2 };
const TILE = 0.36;
const TILE_HEIGHT = 0.06;
const SPACING = 0.34;
const LOOSE_GAP = 0.2;
const ARCHIVE_HEIGHT = 0.16;
const TRAY_HEIGHT = 0.045;
const STACK_ASPECT = 1.33;
// static/img/vfstool-stack.webp: the stack at rest, with a feathered margin around its bounds.
const STILL = { aspect: 1.281, stackWidth: 0.883, stackHeight: 0.85 };
const YAW = -0.3;
const PITCH = 0.62;
const COLLAPSE_EVERY = 26;
// How long a lookup takes to fall from above the stack to its winner.
const FALL = 0.8;
const DWELL = 3.4;
const STILL_PIXELS = 3;

const reduceMotion = matchMedia('(prefers-reduced-motion: reduce)').matches;
const clamp01 = (value) => Math.min(1, Math.max(0, value));
const smooth = (value) => { const t = clamp01(value); return t * t * (3 - 2 * t); };

function cssColor(name, fallback) {
  const raw = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  const color = new THREE.Color(fallback);
  if (raw) {
    try { color.setStyle(raw); } catch { /* an unparsable token keeps the fallback */ }
  }
  return color;
}

function cssFont(name, fallback) {
  const raw = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return raw || fallback;
}

function random(seed) {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// Texture baking -----------------------------------------------------------------------------------

function canvas2d(width, height) {
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  return [canvas, canvas.getContext('2d')];
}

function texture(canvas, colorSpace, anisotropy) {
  const map = new THREE.CanvasTexture(canvas);
  map.colorSpace = colorSpace;
  map.anisotropy = anisotropy;
  map.needsUpdate = true;
  return map;
}

// What each file is, drawn on its tile: texture swatches for textures, a wireframe for a mesh, a
// waveform for a sound, a seal for a plugin, a hex-plated case for an archive.
function drawFace(g, face, s, mono) {
  const rand = random(face * 977 + 13);
  const fill = (color) => { g.fillStyle = color; g.fillRect(0, 0, s, s); };
  const grain = (base, dark, bands, wobble) => {
    fill(base);
    for (let y = 0; y < s; y += 2) {
      const t = y / s;
      const band = Math.sin(t * bands * Math.PI + Math.sin(t * 7 + rand() * 0.2) * wobble) * 0.5 + 0.5;
      g.fillStyle = dark;
      g.globalAlpha = 0.18 + band * 0.42;
      g.fillRect(0, y, s, 2);
    }
    g.globalAlpha = 1;
  };
  const speckle = (count, colors, size) => {
    for (let i = 0; i < count; i++) {
      g.fillStyle = colors[Math.floor(rand() * colors.length)];
      g.globalAlpha = 0.35 + rand() * 0.5;
      const r = size * (0.4 + rand());
      g.fillRect(rand() * s, rand() * s, r, r);
    }
    g.globalAlpha = 1;
  };
  const glyph = (text, color, size, y = 0.54) => {
    g.fillStyle = color;
    g.font = `700 ${Math.round(s * size)}px ${mono}`;
    g.textAlign = 'center';
    g.textBaseline = 'middle';
    g.fillText(text, s / 2, s * y);
  };
  const wire = (color, points, edges) => {
    g.strokeStyle = color;
    g.lineWidth = s / 90;
    g.beginPath();
    for (const [a, b] of edges) {
      g.moveTo(points[a][0] * s, points[a][1] * s);
      g.lineTo(points[b][0] * s, points[b][1] * s);
    }
    g.stroke();
    g.fillStyle = color;
    for (const [x, y] of points) g.fillRect(x * s - s / 80, y * s - s / 80, s / 40, s / 40);
  };
  switch (face) {
    case FACE.wood:
      grain('#8a5a2b', '#3d2410', 9, 2.2);
      speckle(40, ['#c08a4e', '#5a3517'], s / 70);
      break;
    case FACE.stone: {
      fill('#5d5b58');
      for (let i = 0; i < 26; i++) {
        const x = rand() * s; const y = rand() * s; const r = s * (0.07 + rand() * 0.1);
        g.fillStyle = `hsl(30 ${4 + rand() * 6}% ${34 + rand() * 20}%)`;
        g.beginPath(); g.ellipse(x, y, r, r * (0.6 + rand() * 0.4), rand() * 3, 0, Math.PI * 2); g.fill();
        g.strokeStyle = 'rgba(20,18,16,0.7)'; g.lineWidth = s / 60; g.stroke();
      }
      break;
    }
    case FACE.moss:
      fill('#2f4a22');
      speckle(420, ['#5d8a33', '#1d2f14', '#86a84a', '#3a5a2a'], s / 45);
      break;
    case FACE.mesh:
      fill('#0b1a24');
      wire('#6fd6ff', [[0.2, 0.75], [0.5, 0.2], [0.8, 0.75], [0.5, 0.58], [0.34, 0.47], [0.66, 0.47]],
        [[0, 1], [1, 2], [2, 0], [0, 3], [3, 2], [1, 3], [4, 5], [4, 0], [5, 2]]);
      break;
    case FACE.glow: {
      fill('#1a1206');
      const light = g.createRadialGradient(s / 2, s / 2, 0, s / 2, s / 2, s * 0.48);
      light.addColorStop(0, '#fff3c0'); light.addColorStop(0.3, '#ffb347'); light.addColorStop(1, 'rgba(60,30,0,0)');
      g.fillStyle = light; g.fillRect(0, 0, s, s);
      break;
    }
    case FACE.sound:
      fill('#08201c');
      g.strokeStyle = '#5ef0b0'; g.lineWidth = s / 55; g.beginPath();
      for (let x = 0; x <= s; x += 2) {
        const t = x / s;
        const y = 0.5 + Math.sin(t * 38) * 0.28 * Math.sin(t * Math.PI) * (0.6 + 0.4 * Math.sin(t * 7));
        x === 0 ? g.moveTo(x, y * s) : g.lineTo(x, y * s);
      }
      g.stroke();
      break;
    case FACE.lua:
      fill('#15163a');
      glyph('{ }', '#9aa6ff', 0.42, 0.44);
      glyph('lua', '#c6ccff', 0.2, 0.78);
      break;
    case FACE.plugin: {
      fill('#d9c9a0');
      speckle(160, ['#c4b085', '#e8dcbc'], s / 40);
      g.fillStyle = '#8c1c1c';
      g.beginPath(); g.arc(s / 2, s / 2, s * 0.24, 0, Math.PI * 2); g.fill();
      glyph('ES', '#f0d0b0', 0.2);
      break;
    }
    case FACE.archive: {
      fill('#16131c');
      g.strokeStyle = '#e0a44a'; g.lineWidth = s / 70;
      const r = s / 9;
      for (let row = -1; row < 7; row++) {
        for (let col = -1; col < 6; col++) {
          const cx = col * r * 1.75 + (row % 2) * r * 0.875; const cy = row * r * 1.5;
          g.beginPath();
          for (let k = 0; k < 6; k++) { const a = Math.PI / 3 * k + Math.PI / 6; g.lineTo(cx + Math.cos(a) * r * 0.9, cy + Math.sin(a) * r * 0.9); }
          g.closePath(); g.stroke();
        }
      }
      g.fillStyle = 'rgba(22,19,28,0.8)'; g.fillRect(s * 0.18, s * 0.36, s * 0.64, s * 0.28);
      glyph('BSA', '#ffcf7a', 0.2, 0.5);
      break;
    }
    case FACE.rock:
      fill('#6e6254');
      speckle(360, ['#8d7f6c', '#4a4036', '#a89880', '#2e2821'], s / 36);
      g.strokeStyle = 'rgba(25,20,15,0.8)'; g.lineWidth = s / 80;
      for (let i = 0; i < 6; i++) { g.beginPath(); g.moveTo(rand() * s, rand() * s); for (let k = 0; k < 4; k++) g.lineTo(rand() * s, rand() * s); g.stroke(); }
      break;
    case FACE.planks:
      grain('#7a5530', '#3a2410', 4, 0.8);
      g.fillStyle = '#231509';
      for (let i = 1; i < 4; i++) g.fillRect(0, (s / 4) * i - s / 120, s, s / 60);
      break;
    case FACE.gold: {
      fill('#3a2608');
      const gold = g.createRadialGradient(s / 2, s / 2, s * 0.05, s / 2, s / 2, s * 0.5);
      gold.addColorStop(0, '#ffe7a0'); gold.addColorStop(0.5, '#c8902a'); gold.addColorStop(1, '#4a2f08');
      g.fillStyle = gold;
      for (let k = 0; k < 12; k++) {
        g.save(); g.translate(s / 2, s / 2); g.rotate(k * Math.PI / 6);
        g.beginPath(); g.moveTo(0, 0); g.lineTo(s * 0.08, -s * 0.46); g.lineTo(-s * 0.08, -s * 0.46); g.closePath(); g.fill();
        g.restore();
      }
      break;
    }
    case FACE.book:
      fill('#3b1d12');
      g.fillStyle = '#7a3a1c'; g.fillRect(s * 0.2, s * 0.14, s * 0.6, s * 0.72);
      g.strokeStyle = '#e0b060'; g.lineWidth = s / 50; g.strokeRect(s * 0.26, s * 0.2, s * 0.48, s * 0.6);
      glyph('✦', '#f0c870', 0.2, 0.5);
      break;
    case FACE.music:
      fill('#261436');
      glyph('♪', '#e3a8ff', 0.56, 0.5);
      break;
    case FACE.font:
      fill('#1d1d24');
      glyph('Aa', '#f2ecd8', 0.4, 0.52);
      break;
    case FACE.lamp:
      fill('#1c1206');
      wire('#ffb347', [[0.5, 0.12], [0.3, 0.3], [0.7, 0.3], [0.3, 0.78], [0.7, 0.78], [0.5, 0.88], [0.5, 0.52]],
        [[0, 1], [0, 2], [1, 3], [2, 4], [3, 5], [4, 5], [1, 2], [3, 4], [1, 6], [2, 6], [3, 6], [4, 6]]);
      break;
    default:
      fill('#222');
  }
}

function faceAtlas(size, anisotropy, mono) {
  const [canvas, g] = canvas2d(size * 4, size * 4);
  for (let face = 0; face < 16; face++) {
    g.save();
    g.translate((face % 4) * size, Math.floor(face / 4) * size);
    g.beginPath(); g.rect(0, 0, size, size); g.clip();
    drawFace(g, face, size, mono);
    // A bevel: the edge of every tile catches light.
    const bevel = g.createLinearGradient(0, 0, size, size);
    bevel.addColorStop(0, 'rgba(255,255,255,0.16)'); bevel.addColorStop(0.5, 'rgba(255,255,255,0)'); bevel.addColorStop(1, 'rgba(0,0,0,0.3)');
    g.fillStyle = bevel; g.fillRect(0, 0, size, size);
    g.restore();
  }
  const map = texture(canvas, THREE.SRGBColorSpace, anisotropy);
  map.generateMipmaps = true;
  map.minFilter = THREE.LinearMipmapLinearFilter;
  return map;
}

// An archive's lid: hexagonal plates with glowing seams, as colour, normal and emissive maps.
function vaultMaps(size, anisotropy) {
  const [heightCanvas, h] = canvas2d(size, size);
  const [seamCanvas, e] = canvas2d(size, size);
  h.fillStyle = '#fff'; h.fillRect(0, 0, size, size);
  e.fillStyle = '#000'; e.fillRect(0, 0, size, size);
  const r = size / 14;
  for (let row = -1; row < 12; row++) {
    for (let col = -1; col < 10; col++) {
      const cx = col * r * 1.75 + (row % 2) * r * 0.875;
      const cy = row * r * 1.5;
      const path = () => {
        const shape = new Path2D();
        for (let k = 0; k < 6; k++) { const a = Math.PI / 3 * k + Math.PI / 6; shape.lineTo(cx + Math.cos(a) * r * 0.86, cy + Math.sin(a) * r * 0.86); }
        shape.closePath();
        return shape;
      };
      h.strokeStyle = '#000'; h.lineWidth = r * 0.16; h.stroke(path());
      e.strokeStyle = '#fff'; e.lineWidth = r * 0.07; e.stroke(path());
    }
  }
  const heights = h.getImageData(0, 0, size, size).data;
  const [normalCanvas, n] = canvas2d(size, size);
  const image = n.createImageData(size, size);
  const at = (x, y) => heights[(((y + size) % size) * size + ((x + size) % size)) * 4] / 255;
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const dx = (at(x + 1, y) - at(x - 1, y)) * 2.5;
      const dy = (at(x, y + 1) - at(x, y - 1)) * 2.5;
      const length = Math.hypot(dx, dy, 1);
      const i = (y * size + x) * 4;
      image.data[i] = Math.round((-dx / length * 0.5 + 0.5) * 255);
      image.data[i + 1] = Math.round((dy / length * 0.5 + 0.5) * 255);
      image.data[i + 2] = Math.round((1 / length * 0.5 + 0.5) * 255);
      image.data[i + 3] = 255;
    }
  }
  n.putImageData(image, 0, 0);
  return {
    normal: texture(normalCanvas, THREE.NoColorSpace, anisotropy),
    seams: texture(seamCanvas, THREE.SRGBColorSpace, anisotropy),
  };
}

// A tray's floor: an outlined slot for every cell of the grid, the directory's empty places.
function slotMap(cols, rows, anisotropy) {
  const size = 128;
  const [canvas, g] = canvas2d(cols * size, rows * size);
  g.clearRect(0, 0, canvas.width, canvas.height);
  g.strokeStyle = 'rgba(255,255,255,0.55)';
  g.lineWidth = 3;
  g.setLineDash([10, 8]);
  const inset = size * (1 - TILE / CELL) / 2;
  for (let row = 0; row < rows; row++) {
    for (let col = 0; col < cols; col++) g.strokeRect(col * size + inset, row * size + inset, size - inset * 2, size - inset * 2);
  }
  return texture(canvas, THREE.SRGBColorSpace, anisotropy);
}

function labelCanvas(width, height) {
  const [canvas, g] = canvas2d(width, height);
  return { canvas, g, map: texture(canvas, THREE.SRGBColorSpace, 4) };
}

// The environment the glass and metal reflect: a dark room with a few long light panels.
function environment(renderer, accent, info) {
  const room = new THREE.Scene();
  const shell = new THREE.Mesh(new THREE.SphereGeometry(10, 32, 16), new THREE.MeshBasicMaterial({ color: new THREE.Color(0.012, 0.012, 0.03), side: THREE.BackSide }));
  room.add(shell);
  const panel = (color, strength, x, y, z, w, h) => {
    const mesh = new THREE.Mesh(new THREE.PlaneGeometry(w, h), new THREE.MeshBasicMaterial({ color: color.clone().multiplyScalar(strength), side: THREE.DoubleSide }));
    mesh.position.set(x, y, z);
    mesh.lookAt(0, 0, 0);
    room.add(mesh);
  };
  panel(new THREE.Color(1, 1, 1), 3.2, 0, 7, 2, 8, 1.2);
  panel(accent, 4.5, -7, 2, 3, 1.2, 6);
  panel(info, 3.0, 7, 1, -2, 1.2, 5);
  panel(new THREE.Color(0.8, 0.85, 1), 1.6, 0, -2, 8, 10, 0.6);
  const generator = new THREE.PMREMGenerator(renderer);
  const target = generator.fromScene(room, 0.03);
  generator.dispose();
  return target;
}

// Shaders ------------------------------------------------------------------------------------------

const FULLSCREEN_VERTEX = /* glsl */ `
  varying vec2 vUv;
  void main() {
    vUv = uv;
    gl_Position = vec4(position.xy, 0.0, 1.0);
  }
`;

const HASH = /* glsl */ `
  float hash(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
`;

// The void: the page's own colours, a glow behind the stack, and faint lookups falling past it.
const SKY_FRAGMENT = /* glsl */ `
  uniform vec3 uTop;
  uniform vec3 uBottom;
  uniform vec3 uGlow;
  uniform vec3 uRain;
  uniform vec2 uCenter;
  uniform vec2 uResolution;
  uniform float uRadius;
  uniform float uTime;
  varying vec2 vUv;
  ${HASH}
  void main() {
    vec3 color = mix(uBottom, uTop, vUv.y);
    vec2 aspect = vec2(uResolution.x / max(uResolution.y, 1.0), 1.0);
    float d = length((vUv - uCenter) * aspect) / max(uRadius, 1e-3);
    color += uGlow * exp(-d * d * 0.9);
    vec2 cell = vec2(floor(vUv.x * uResolution.x / 14.0), 0.0);
    float column = hash(cell);
    float speed = 0.04 + column * 0.08;
    float drop = fract(vUv.y + uTime * speed + column * 7.0);
    float streak = smoothstep(0.0, 0.2, drop) * (1.0 - smoothstep(0.2, 0.26, drop));
    float lane = step(0.82, column) * (1.0 - smoothstep(0.0, 0.35, abs(fract(vUv.x * uResolution.x / 14.0) - 0.5)));
    color += uRain * streak * lane * 0.05 * exp(-d * 0.35);
    gl_FragColor = vec4(color, 1.0);
  }
`;

// The membrane between the archives and the loose files: loose files always win.
const MEMBRANE_VERTEX = /* glsl */ `
  varying vec2 vUv;
  void main() {
    vUv = uv;
    gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
  }
`;
const MEMBRANE_FRAGMENT = /* glsl */ `
  uniform vec3 uColor;
  uniform float uTime;
  uniform float uOpacity;
  varying vec2 vUv;
  float hexDistance(vec2 p) {
    p = abs(p);
    return max(dot(p, normalize(vec2(1.0, 1.7320508))), p.x);
  }
  void main() {
    vec2 p = (vUv - 0.5) * vec2(11.0, 9.0);
    vec2 grid = vec2(1.0, 1.7320508);
    vec2 a = mod(p, grid) - grid * 0.5;
    vec2 b = mod(p - grid * 0.5, grid) - grid * 0.5;
    vec2 cell = dot(a, a) < dot(b, b) ? a : b;
    float edge = smoothstep(0.38, 0.5, hexDistance(cell));
    float wave = 0.5 + 0.5 * sin(p.x * 0.7 + p.y * 0.4 - uTime * 1.1);
    float border = min(min(vUv.x, 1.0 - vUv.x), min(vUv.y, 1.0 - vUv.y));
    float fade = smoothstep(0.0, 0.18, border);
    float rim = 1.0 - smoothstep(0.0, 0.025, border);
    float alpha = (edge * (0.14 + 0.2 * wave) + 0.03) * fade + rim * 0.22;
    gl_FragColor = vec4(uColor * alpha * uOpacity, 1.0);
  }
`;

// A lookup: a thread of light falling from above, brightest at its head.
const BEAM_VERTEX = /* glsl */ `
  varying float vAlong;
  varying float vFacing;
  void main() {
    vAlong = position.y + 0.5;
    vec4 view = modelViewMatrix * vec4(position, 1.0);
    vec3 n = normalMatrix * normal;
    vec3 toEye = -view.xyz;
    vFacing = abs(dot(n, toEye)) / max(length(n) * length(toEye), 1e-4);
    gl_Position = projectionMatrix * view;
  }
`;
const BEAM_FRAGMENT = /* glsl */ `
  uniform vec3 uColor;
  uniform float uIntensity;
  varying float vAlong;
  varying float vFacing;
  void main() {
    float head = 1.0 - smoothstep(0.0, 0.55, vAlong);
    float tail = smoothstep(0.0, 1.0, vAlong);
    float glow = 0.25 + head * 2.2 + (1.0 - tail) * 0.3;
    float core = vFacing * vFacing * vFacing;
    gl_FragColor = vec4(uColor * glow * uIntensity * core, 1.0);
  }
`;

// Rings: a lookup's landing, and the charge of a held pointer, drawn as an arc.
const RING_VERTEX = MEMBRANE_VERTEX;
const RING_FRAGMENT = /* glsl */ `
  uniform vec3 uColor;
  uniform float uRadius;
  uniform float uWidth;
  uniform float uArc;
  uniform float uIntensity;
  varying vec2 vUv;
  void main() {
    vec2 p = vUv - 0.5;
    float r = length(p) * 2.0;
    float band = 1.0 - smoothstep(0.0, uWidth, abs(r - uRadius));
    float angle = atan(p.x, p.y) / 6.2831853 + 0.5;
    float arc = 1.0 - smoothstep(uArc - 0.004, uArc + 0.004, angle);
    gl_FragColor = vec4(uColor * band * arc * uIntensity, 1.0);
  }
`;

// Bytes adrift around the stack: hex digits and path separators.
const GLYPH_VERTEX = /* glsl */ `
  attribute vec4 aSeed;
  uniform float uTime;
  uniform float uScale;
  uniform float uPixel;
  uniform vec3 uCenter;
  varying float vFade;
  varying float vGlyph;
  void main() {
    float life = fract(aSeed.w + uTime * (0.012 + aSeed.z * 0.02));
    float angle = aSeed.x * 6.2831853 + uTime * 0.03 * (aSeed.z - 0.5);
    float radius = (1.2 + aSeed.y * 2.6) * uScale;
    vec3 p = uCenter + vec3(cos(angle) * radius, (life - 0.5) * 4.2 * uScale, sin(angle) * radius * 0.6 - 0.6 * uScale);
    vFade = smoothstep(0.0, 0.15, life) * (1.0 - smoothstep(0.75, 1.0, life));
    vGlyph = floor(aSeed.z * 19.0);
    vec4 view = modelViewMatrix * vec4(p, 1.0);
    gl_Position = projectionMatrix * view;
    gl_PointSize = clamp(uPixel * 0.16 * uScale / max(-view.z, 0.1), 4.0, 40.0);
  }
`;
const GLYPH_FRAGMENT = /* glsl */ `
  uniform sampler2D tGlyphs;
  uniform vec3 uColor;
  varying float vFade;
  varying float vGlyph;
  void main() {
    vec2 cell = vec2(mod(vGlyph, 5.0), floor(vGlyph / 5.0));
    vec2 uv = (cell + vec2(gl_PointCoord.x, 1.0 - gl_PointCoord.y)) / vec2(5.0, 4.0);
    float a = texture2D(tGlyphs, uv).a;
    gl_FragColor = vec4(uColor * a * vFade * 0.3, 1.0);
  }
`;

// Any NaN or infinity a driver produces is zeroed and bright values capped before the bloom, which
// would otherwise smear a single bad pixel into a black square.
const SCRUB = /* glsl */ `
  vec3 scrub(vec3 c) {
    if (any(isnan(c)) || any(isinf(c)) || c.r != c.r || c.g != c.g || c.b != c.b) return vec3(0.0);
    return clamp(c, 0.0, 64.0);
  }
`;

const BRIGHT_FRAGMENT = /* glsl */ `
  uniform sampler2D tInput;
  uniform float uThreshold;
  varying vec2 vUv;
  ${SCRUB}
  void main() {
    vec3 c = scrub(texture2D(tInput, vUv).rgb);
    float luma = dot(c, vec3(0.2126, 0.7152, 0.0722));
    gl_FragColor = vec4(c * smoothstep(uThreshold, uThreshold + 0.7, luma), 1.0);
  }
`;

const BLUR_FRAGMENT = /* glsl */ `
  uniform sampler2D tInput;
  uniform vec2 uDirection;
  varying vec2 vUv;
  void main() {
    vec3 sum = texture2D(tInput, vUv).rgb * 0.2270270270;
    sum += texture2D(tInput, vUv + uDirection * 1.3846153846).rgb * 0.3162162162;
    sum += texture2D(tInput, vUv - uDirection * 1.3846153846).rgb * 0.3162162162;
    sum += texture2D(tInput, vUv + uDirection * 3.2307692308).rgb * 0.0702702703;
    sum += texture2D(tInput, vUv - uDirection * 3.2307692308).rgb * 0.0702702703;
    gl_FragColor = vec4(sum, 1.0);
  }
`;

const COMPOSITE_FRAGMENT = /* glsl */ `
  uniform sampler2D tScene;
  uniform sampler2D tBloomNear;
  uniform sampler2D tBloomFar;
  uniform vec4 uScrim;
  uniform vec2 uResolution;
  uniform float uTime;
  varying vec2 vUv;
  vec3 aces(vec3 x) {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), 0.0, 1.0);
  }
  float dither(vec2 p) {
    return fract(sin(dot(p + fract(uTime), vec2(12.9898, 78.233))) * 43758.5453) - 0.5;
  }
  ${SCRUB}
  void main() {
    vec3 color = scrub(texture2D(tScene, vUv).rgb);
    color += scrub(texture2D(tBloomNear, vUv).rgb) * 0.5 + scrub(texture2D(tBloomFar, vUv).rgb) * 0.4;
    vec2 pixel = vUv * uResolution;
    vec2 inside = min(pixel - uScrim.xy, uScrim.zw - pixel);
    float scrim = smoothstep(-60.0, 40.0, min(inside.x, inside.y));
    color *= 1.0 - scrim * 0.55;
    color = aces(color * 0.95);
    color = pow(color, vec3(1.0 / 2.2));
    color += dither(gl_FragCoord.xy) / 255.0;
    gl_FragColor = vec4(color, 1.0);
  }
`;

function fullscreenMaterial(fragmentShader, uniforms) {
  return new THREE.ShaderMaterial({ vertexShader: FULLSCREEN_VERTEX, fragmentShader, uniforms, depthTest: false, depthWrite: false });
}

function additive(vertexShader, fragmentShader, uniforms) {
  return new THREE.ShaderMaterial({
    vertexShader, fragmentShader, uniforms,
    transparent: true, depthWrite: false, blending: THREE.AdditiveBlending, side: THREE.DoubleSide,
  });
}

// The tiles: one instanced box per copy of a file. Each carries its face in the atlas, whether it
// wins (1) or is overridden (0), a glow, and its hover lift. The atlas covers each tile's top; its
// sides take the face's colour, darker. A winner's rim burns in the accent; an overridden copy is
// desaturated and cooled.
function tileMaterial(atlas, accent, dim) {
  const material = new THREE.MeshStandardMaterial({ map: atlas, roughness: 0.68, metalness: 0.02, envMapIntensity: 0.45 });
  material.onBeforeCompile = (shader) => {
    shader.uniforms.uAccent = { value: accent };
    shader.uniforms.uDim = dim;
    shader.vertexShader = shader.vertexShader
      .replace('#include <common>', '#include <common>\nattribute vec4 aTile;\nvarying vec4 vTile;\nvarying vec2 vLocal;\nvarying float vTop;')
      .replace('#include <uv_vertex>', `#include <uv_vertex>
        vTile = aTile;
        vLocal = uv;
        vTop = step(0.5, normal.y);
        vec2 cellXY = vec2(mod(aTile.x, 4.0), 3.0 - floor(aTile.x / 4.0));
        vec2 local = mix(vec2(0.5), uv, vTop);
        vMapUv = (cellXY + 0.03 + local * 0.94) / 4.0;`);
    shader.fragmentShader = shader.fragmentShader
      .replace('#include <common>', '#include <common>\nuniform vec3 uAccent;\nuniform float uDim;\nvarying vec4 vTile;\nvarying vec2 vLocal;\nvarying float vTop;')
      .replace('#include <map_fragment>', `#include <map_fragment>
        float ghost = 1.0 - vTile.y;
        float grey = dot(diffuseColor.rgb, vec3(0.299, 0.587, 0.114));
        diffuseColor.rgb = mix(diffuseColor.rgb, grey * vec3(0.5, 0.56, 0.86) * 0.5, ghost * 0.82 * (1.0 - uDim * 0.55 * vTile.w));
        diffuseColor.rgb *= mix(0.5, 1.0, vTop);
        diffuseColor.rgb *= 1.0 - uDim * 0.75 * (1.0 - vTile.w);`)
      .replace('#include <emissivemap_fragment>', `#include <emissivemap_fragment>
        float border = min(min(vLocal.x, 1.0 - vLocal.x), min(vLocal.y, 1.0 - vLocal.y));
        float rim = vTop * (1.0 - smoothstep(0.0, 0.07, border));
        totalEmissiveRadiance += uAccent * rim * vTile.y * 0.7;
        totalEmissiveRadiance += diffuseColor.rgb * vTile.y * vTop * 0.16;
        totalEmissiveRadiance += (uAccent * 0.7 + vec3(0.35)) * vTile.z * (0.35 + vTop * 0.9);
        totalEmissiveRadiance *= 1.0 - uDim * 0.8 * (1.0 - vTile.w);`);
  };
  material.customProgramCacheKey = () => 'vfstool-tile';
  return material;
}

// Layout -------------------------------------------------------------------------------------------

function textRects(text) {
  const rects = [];
  const range = document.createRange();
  const walker = document.createTreeWalker(text, NodeFilter.SHOW_TEXT, {
    acceptNode: (node) => (node.nodeValue.trim() ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT),
  });
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    range.selectNodeContents(node);
    for (const rect of range.getClientRects()) rects.push(rect);
  }
  for (const element of text.querySelectorAll('a, button, input, select, svg, .dw-command, .dw-badge')) rects.push(element.getBoundingClientRect());
  return rects.filter((rect) => rect.width > 0 && rect.height > 0);
}

// The largest square clear of the text: beside all of it, beside the title rows above the summary,
// or above it all where the stylesheet has left room on a phone. Returns it relative to the art,
// with the text's bounds for the scrim.
function placement(root) {
  const hero = root.closest('.dw-hero') || root.parentElement;
  const box = root.getBoundingClientRect();
  const text = hero.querySelector('.dw-hero__text') || hero.querySelector('.dw-shell');
  const strip = hero.querySelector('.dw-strip');
  const shellElement = hero.querySelector('.dw-hero__grid') || hero.querySelector('.dw-shell') || hero;
  const shellStyle = getComputedStyle(shellElement);
  const shellBox = shellElement.getBoundingClientRect();
  const shell = strip ? strip.getBoundingClientRect() : { left: shellBox.left + parseFloat(shellStyle.paddingLeft), right: shellBox.right - parseFloat(shellStyle.paddingRight) };
  const summary = hero.querySelector('.dw-hero__summary');
  const floor = strip ? strip.getBoundingClientRect().top : box.bottom - 24;
  const rects = text ? textRects(text) : [];
  const fallback = { x: box.width * 0.75, y: box.height * 0.45, size: Math.min(box.width * 0.3, box.height * 0.7), above: false, text: [0, 0, 0, 0] };
  if (!rects.length) return fallback;
  const gap = 32;
  const right = Math.max(...rects.map((rect) => rect.right));
  const left = Math.min(...rects.map((rect) => rect.left));
  const top = Math.min(...rects.map((rect) => rect.top));
  const bottom = Math.max(...rects.map((rect) => rect.bottom));
  const summaryTop = summary ? summary.getBoundingClientRect().top : floor;
  const headRects = rects.filter((rect) => rect.bottom <= summaryTop + 1);
  const headRight = headRects.length ? Math.max(...headRects.map((rect) => rect.right)) : right;
  const candidates = [
    { x0: right + gap, x1: shell.right, y0: box.top + 12, y1: floor - 12, above: false },
    { x0: headRight + gap, x1: shell.right, y0: box.top + 12, y1: summaryTop - 12, above: false },
    { x0: shell.left, x1: shell.right, y0: box.top + 8, y1: top - 10, above: true },
  ].map((region) => {
    const width = region.x1 - region.x0;
    const height = region.y1 - region.y0;
    return { ...region, size: Math.max(0, Math.min(height, width / STACK_ASPECT)) };
  });
  const best = candidates.reduce((a, b) => (b.size > a.size ? b : a));
  const regionWidth = Math.min(best.x1 - best.x0, 820);
  const regionHeight = Math.min(best.y1 - best.y0, 560);
  const x = best.above ? (best.x0 + best.x1) / 2 : best.x1 - regionWidth / 2;
  return {
    x: x - box.left,
    y: (best.y0 + best.y1) / 2 - box.top,
    size: Math.min(best.size, 560),
    width: regionWidth,
    height: regionHeight,
    above: best.above,
    text: [left - box.left, top - box.top, right - box.left, bottom - box.top],
  };
}

// The stack -----------------------------------------------------------------------------------------

function slabBase(index) {
  return index * SPACING + (index >= ARCHIVES ? LOOSE_GAP : 0);
}
const STACK_MIDDLE = (slabBase(SOURCES.length - 1) + slabBase(0)) / 2;

function mount(root) {
  const still = document.createElement('img');
  still.className = 'vfs-hero__still';
  still.alt = '';
  still.decoding = 'async';
  still.src = new URL('../img/vfstool-stack.webp', import.meta.url).href;
  root.append(still);

  const canvas = document.createElement('canvas');
  canvas.className = 'vfs-hero__canvas';
  // Ask for WebGL 2 first, so a browser without it keeps the still and a quiet console.
  const context = canvas.getContext('webgl2', { antialias: false, alpha: false, depth: true, stencil: false, powerPreference: 'high-performance' });
  if (!context) {
    placeStill();
    return;
  }
  let renderer;
  try {
    renderer = new THREE.WebGLRenderer({ canvas, context });
  } catch {
    placeStill();
    return;
  }
  renderer.autoClear = false;
  renderer.outputColorSpace = THREE.LinearSRGBColorSpace;
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  root.append(canvas);

  const small = Math.min(innerWidth, innerHeight) < 700;
  const cols = small ? 4 : 7;
  const rows = 3;
  const keys = KEYS.filter((entry) => entry.col < cols && entry.row < rows);

  const floatTargets = renderer.extensions.has('EXT_color_buffer_float') || renderer.extensions.has('EXT_color_buffer_half_float');
  const targetType = floatTargets ? THREE.HalfFloatType : THREE.UnsignedByteType;
  const makeTarget = () => new THREE.WebGLRenderTarget(1, 1, { type: targetType, depthBuffer: false });
  const sceneTarget = new THREE.WebGLRenderTarget(1, 1, { type: targetType, samples: 4 });
  const bloomTargets = [makeTarget(), makeTarget(), makeTarget(), makeTarget()];
  const anisotropy = Math.min(8, renderer.capabilities.getMaxAnisotropy());
  const mono = cssFont('--dw-font-mono', 'ui-monospace, "DejaVu Sans Mono", monospace');

  const accent = cssColor('--dw-accent', '#9aa6ff');
  const info = cssColor('--dw-info', '#79c0ff');
  const warn = cssColor('--dw-warn', '#e0a44a');
  const top = cssColor('--dw-bg-1', '#12121f');
  const bottom = cssColor('--dw-bg-0', '#0b0b14');
  const text = cssColor('--dw-text', '#e8e8f4');

  const camera = new THREE.PerspectiveCamera(30, 1, 0.1, 80);
  camera.position.set(0, 0, 11);
  camera.lookAt(0, 0, 0);

  const scene = new THREE.Scene();
  const envTarget = environment(renderer, accent, info);
  scene.environment = envTarget.texture;

  const quad = new THREE.PlaneGeometry(2, 2);
  const skyUniforms = {
    uTop: { value: top },
    uBottom: { value: bottom },
    uGlow: { value: accent.clone().multiplyScalar(0.12) },
    uRain: { value: info.clone() },
    uCenter: { value: new THREE.Vector2(0.75, 0.5) },
    uResolution: { value: new THREE.Vector2(1, 1) },
    uRadius: { value: 0.3 },
    uTime: { value: 0 },
  };
  const sky = new THREE.Mesh(quad, fullscreenMaterial(SKY_FRAGMENT, skyUniforms));
  sky.frustumCulled = false;
  sky.renderOrder = -10;
  scene.add(sky);

  // The stack's frame: `stack` takes the pose and scale, `body` is centred on the stack's middle.
  const stack = new THREE.Group();
  const body = new THREE.Group();
  body.position.y = -STACK_MIDDLE;
  stack.add(body);
  scene.add(stack);

  const width = cols * CELL + 0.26;
  const depth = rows * CELL + 0.26;
  const cellX = (col) => (col - (cols - 1) / 2) * CELL;
  const cellZ = (row) => (row - (rows - 1) / 2) * CELL;

  // The archives: dark hex-plated vaults with glowing seams.
  const vault = vaultMaps(small ? 256 : 512, anisotropy);
  vault.normal.wrapS = vault.normal.wrapT = THREE.RepeatWrapping;
  vault.seams.wrapS = vault.seams.wrapT = THREE.RepeatWrapping;
  vault.normal.repeat.set(0.8, 0.7);
  vault.seams.repeat.set(0.8, 0.7);
  const slabs = [];
  const slots = slotMap(cols, rows, anisotropy);
  for (let index = 0; index < SOURCES.length; index++) {
    const source = SOURCES[index];
    const archive = source.kind === 'archive';
    const group = new THREE.Group();
    let mesh;
    if (archive) {
      mesh = new THREE.Mesh(new THREE.BoxGeometry(width, ARCHIVE_HEIGHT, depth), new THREE.MeshPhysicalMaterial({
        color: new THREE.Color(0.06, 0.055, 0.08),
        metalness: 0.55,
        roughness: 0.55,
        normalMap: vault.normal,
        normalScale: new THREE.Vector2(0.8, 0.8),
        clearcoat: 0.15,
        clearcoatRoughness: 0.6,
        emissive: warn.clone().multiplyScalar(index === 0 ? 0.9 : 0.7),
        emissiveMap: vault.seams,
        emissiveIntensity: 0.1,
      }));
      mesh.position.y = -ARCHIVE_HEIGHT / 2;
      mesh.castShadow = false;
      mesh.receiveShadow = true;
    } else {
      mesh = new THREE.Mesh(new THREE.BoxGeometry(width, TRAY_HEIGHT, depth), new THREE.MeshPhysicalMaterial({
        color: new THREE.Color(0.2, 0.22, 0.42).lerp(accent, 0.18),
        metalness: 0,
        roughness: 0.42,
        transparent: true,
        opacity: index === 3 ? 0.05 : 0.09,
        clearcoat: 0.25,
        clearcoatRoughness: 0.5,
        envMapIntensity: 0.3,
        depthWrite: false,
      }));
      mesh.position.y = -TRAY_HEIGHT / 2;
      mesh.receiveShadow = true;
      mesh.renderOrder = 2;
      const floor = new THREE.Mesh(new THREE.PlaneGeometry(cols * CELL, rows * CELL), new THREE.MeshBasicMaterial({
        map: slots, transparent: true, opacity: 0.16, depthWrite: false, color: accent,
      }));
      floor.rotation.x = -Math.PI / 2;
      floor.position.y = 0.002;
      floor.renderOrder = 3;
      group.add(floor);
    }
    group.add(mesh);
    const rim = new THREE.LineSegments(new THREE.EdgesGeometry(mesh.geometry), new THREE.LineBasicMaterial({
      color: (archive ? warn : accent).clone().multiplyScalar(index === 3 ? 0.6 : 1.15),
      transparent: true,
      opacity: index === 3 ? 0.3 : 0.75,
      depthWrite: false,
    }));
    rim.position.copy(mesh.position);
    rim.renderOrder = 4;
    group.add(rim);

    // The source's number and name on its front edge, as the reports list it.
    const label = labelCanvas(1024, 96);
    label.g.font = `600 52px ${mono}`;
    label.g.textBaseline = 'middle';
    label.g.fillStyle = `#${(archive ? warn : accent).getHexString()}`;
    label.g.fillText(String(index), 8, 50);
    label.g.fillStyle = index === 3 ? 'rgba(220,224,255,0.45)' : 'rgba(236,238,255,0.92)';
    label.g.fillText(source.name, 70, 50);
    label.g.font = `400 34px ${mono}`;
    label.g.fillStyle = 'rgba(200,205,240,0.5)';
    label.g.fillText(source.kind, 70 + label.g.measureText(source.name).width * (52 / 34) + 30, 54);
    label.map.needsUpdate = true;
    const tag = new THREE.Mesh(new THREE.PlaneGeometry(1.6, 0.15), new THREE.MeshBasicMaterial({ map: label.map, transparent: true, depthWrite: false, toneMapped: false }));
    tag.position.set(-width / 2 + 0.8, archive ? -ARCHIVE_HEIGHT * 0.55 : -0.03, depth / 2 + 0.012);
    tag.renderOrder = 6;
    group.add(tag);

    group.position.y = slabBase(index);
    body.add(group);
    slabs.push({ group, mesh, rim, tag, archive, index });
  }

  // The membrane between archives and loose files.
  const membraneUniforms = { uColor: { value: info.clone().lerp(accent, 0.4).multiplyScalar(1.1) }, uTime: { value: 0 }, uOpacity: { value: 1 } };
  const membrane = new THREE.Mesh(new THREE.PlaneGeometry(width + 0.24, depth + 0.24), additive(MEMBRANE_VERTEX, MEMBRANE_FRAGMENT, membraneUniforms));
  membrane.rotation.x = -Math.PI / 2;
  membrane.position.y = slabBase(ARCHIVES) - LOOSE_GAP / 2 - SPACING / 2 + 0.05;
  membrane.renderOrder = 1;
  body.add(membrane);

  // The collapsed folder: one tray that holds every winner, shown while the stack is closed.
  const merged = new THREE.Mesh(new THREE.BoxGeometry(width + 0.1, TRAY_HEIGHT, depth + 0.1), new THREE.MeshPhysicalMaterial({
    color: accent, metalness: 0, roughness: 0.1, transparent: true, opacity: 0, clearcoat: 1, envMapIntensity: 1.8, depthWrite: false,
    emissive: accent, emissiveIntensity: 0,
  }));
  merged.receiveShadow = true;
  const mergedLabel = labelCanvas(1024, 96);
  mergedLabel.g.textBaseline = 'middle';
  mergedLabel.g.font = `600 50px ${mono}`;
  mergedLabel.g.fillStyle = 'rgba(210,214,240,0.7)';
  mergedLabel.g.fillText('$', 8, 50);
  mergedLabel.g.fillStyle = `#${info.getHexString()}`;
  mergedLabel.g.fillText('vfstool collapse', 52, 50);
  mergedLabel.map.needsUpdate = true;
  const mergedTag = new THREE.Mesh(new THREE.PlaneGeometry(1.6, 0.15), new THREE.MeshBasicMaterial({ map: mergedLabel.map, transparent: true, depthWrite: false, toneMapped: false, opacity: 0 }));
  mergedTag.position.set(-width / 2 + 0.85, -0.05, depth / 2 + 0.07);
  mergedTag.renderOrder = 6;
  merged.add(mergedTag);
  merged.material.roughness = 0.4;
  merged.material.clearcoat = 0.2;
  merged.material.envMapIntensity = 0.4;
  merged.renderOrder = 2;
  body.add(merged);

  // The tiles.
  const atlas = faceAtlas(small ? 128 : 256, anisotropy, mono);
  const tiles = [];
  for (const entry of keys) {
    const winner = entry.from[entry.from.length - 1];
    for (const source of entry.from) {
      tiles.push({ entry, source, winner: source === winner, lift: 0, glow: 0, explain: entry.key === EXPLAIN_KEY });
    }
  }
  const tileGeometry = new THREE.BoxGeometry(TILE, TILE_HEIGHT, TILE);
  const tileData = new Float32Array(tiles.length * 4);
  tileGeometry.setAttribute('aTile', new THREE.InstancedBufferAttribute(tileData, 4));
  const dim = { value: 0 };
  const tileMesh = new THREE.InstancedMesh(tileGeometry, tileMaterial(atlas, accent, dim), tiles.length);
  tileMesh.castShadow = true;
  tileMesh.receiveShadow = true;
  tileMesh.frustumCulled = false;
  body.add(tileMesh);
  tiles.forEach((tile, i) => {
    tileData[i * 4] = tile.entry.face;
    tileData[i * 4 + 1] = tile.winner ? 1 : 0;
    tileData[i * 4 + 3] = tile.explain ? 1 : 0;
    tile.home = new THREE.Vector3(cellX(tile.entry.col), 0, cellZ(tile.entry.row));
  });
  const target = tiles.findIndex((tile) => tile.explain && tile.winner);
  const explainTiles = tiles.map((tile, i) => ({ tile, i })).filter(({ tile }) => tile.explain);

  // The light straight above casts every winner's shadow on the copies below it.
  const sun = new THREE.DirectionalLight(new THREE.Color(1, 0.97, 0.92), 2.0);
  sun.position.set(0.25, 9, 0.45);
  sun.target.position.set(0, 0, 0);
  sun.castShadow = true;
  sun.shadow.mapSize.set(small ? 1024 : 2048, small ? 1024 : 2048);
  sun.shadow.bias = -0.0006;
  sun.shadow.radius = 3;
  // The shadow camera works in world units, so it follows the stack's scale.
  function fitShadow() {
    const reach = Math.max(width, depth) * 0.95 * scale;
    Object.assign(sun.shadow.camera, { left: -reach, right: reach, top: reach, bottom: -reach, near: 5 * scale, far: 14 * scale });
    sun.shadow.normalBias = 0.01 * scale;
    sun.shadow.camera.updateProjectionMatrix();
  }
  body.add(sun, sun.target);
  const fill = new THREE.DirectionalLight(info, 0.8);
  fill.position.set(-5, 2, 4);
  const rimLight = new THREE.DirectionalLight(accent, 1.6);
  rimLight.position.set(4, 3, -6);
  const lamp = new THREE.PointLight(new THREE.Color(0.9, 0.92, 1.0), 0, 0, 2);
  scene.add(fill, rimLight, lamp);
  scene.add(new THREE.HemisphereLight(accent.clone().multiplyScalar(0.25), bottom.clone(), 0.5));

  // Lookups: a beam falls down a key's column and lands on its winner.
  const beamUniforms = { uColor: { value: info.clone().lerp(new THREE.Color(1, 1, 1), 0.35) }, uIntensity: { value: 0 } };
  const beam = new THREE.Mesh(new THREE.CylinderGeometry(0.03, 0.03, 1, 12, 1, true), additive(BEAM_VERTEX, BEAM_FRAGMENT, beamUniforms));
  beam.renderOrder = 8;
  const haloUniforms = { uColor: { value: info.clone().multiplyScalar(0.35) }, uIntensity: { value: 0 } };
  const halo = new THREE.Mesh(new THREE.CylinderGeometry(0.11, 0.11, 1, 16, 1, true), additive(BEAM_VERTEX, BEAM_FRAGMENT, haloUniforms));
  halo.renderOrder = 8;
  beam.add(halo);
  body.add(beam);
  const landingUniforms = { uColor: { value: info.clone().multiplyScalar(2.2) }, uRadius: { value: 0.3 }, uWidth: { value: 0.08 }, uArc: { value: 1.01 }, uIntensity: { value: 0 } };
  const landing = new THREE.Mesh(new THREE.PlaneGeometry(1, 1), additive(RING_VERTEX, RING_FRAGMENT, landingUniforms));
  landing.rotation.x = -Math.PI / 2;
  landing.renderOrder = 8;
  body.add(landing);

  // The held pointer's charge, drawn around the explained tile.
  const chargeUniforms = { uColor: { value: accent.clone().multiplyScalar(2.2) }, uRadius: { value: 0.88 }, uWidth: { value: 0.05 }, uArc: { value: 0 }, uIntensity: { value: 0 } };
  const charge = new THREE.Mesh(new THREE.PlaneGeometry(TILE * 1.9, TILE * 1.9), additive(RING_VERTEX, RING_FRAGMENT, chargeUniforms));
  charge.rotation.x = -Math.PI / 2;
  charge.renderOrder = 9;
  body.add(charge);

  // Bytes adrift.
  const [glyphCanvas, glyphs] = canvas2d(320, 256);
  glyphs.font = `700 48px ${mono}`;
  glyphs.textAlign = 'center';
  glyphs.textBaseline = 'middle';
  glyphs.fillStyle = '#fff';
  '0123456789ABCDEF/\\.:'.split('').forEach((character, i) => glyphs.fillText(character, (i % 5) * 64 + 32, Math.floor(i / 5) * 64 + 34));
  const glyphCount = small ? 50 : 110;
  const glyphGeometry = new THREE.BufferGeometry();
  const seeds = new Float32Array(glyphCount * 4);
  const seedRandom = random(417);
  for (let i = 0; i < seeds.length; i++) seeds[i] = seedRandom();
  glyphGeometry.setAttribute('position', new THREE.BufferAttribute(new Float32Array(glyphCount * 3), 3));
  glyphGeometry.setAttribute('aSeed', new THREE.BufferAttribute(seeds, 4));
  const glyphUniforms = {
    tGlyphs: { value: texture(glyphCanvas, THREE.NoColorSpace, 1) },
    uColor: { value: accent.clone().lerp(info, 0.5) },
    uTime: { value: 0 },
    uScale: { value: 1 },
    uPixel: { value: 400 },
    uCenter: { value: new THREE.Vector3() },
  };
  const drift = new THREE.Points(glyphGeometry, additive(GLYPH_VERTEX, GLYPH_FRAGMENT, glyphUniforms));
  drift.frustumCulled = false;
  drift.renderOrder = 7;
  scene.add(drift);

  // `explain`: labels for each provider of the key, and the query above them.
  const labels = explainTiles.map(({ tile }) => {
    const label = labelCanvas(1024, 192);
    const mesh = new THREE.Mesh(new THREE.PlaneGeometry(2.9, 0.544), new THREE.MeshBasicMaterial({ map: label.map, transparent: true, depthWrite: false, depthTest: false, toneMapped: false, opacity: 0 }));
    mesh.renderOrder = 20;
    body.add(mesh);
    return { ...label, mesh, source: tile.source, winner: tile.winner, drawn: '' };
  });
  // The pane `explain` prints into, behind the providers and their labels.
  const paneCanvas = labelCanvas(1024, 696);
  {
    const { g } = paneCanvas;
    const radius = 36;
    g.beginPath();
    g.roundRect(6, 6, 1012, 684, radius);
    g.fillStyle = `rgba(${Math.round(bottom.r * 255)},${Math.round(bottom.g * 255)},${Math.round(bottom.b * 255)},0.9)`;
    g.fill();
    g.lineWidth = 4;
    g.strokeStyle = `#${accent.getHexString()}88`;
    g.stroke();
    g.fillStyle = `#${accent.getHexString()}33`;
    g.beginPath();
    g.roundRect(6, 6, 1012, 84, [radius, radius, 0, 0]);
    g.fill();
    paneCanvas.map.needsUpdate = true;
  }
  const pane = new THREE.Mesh(new THREE.PlaneGeometry(4.0, 2.72), new THREE.MeshBasicMaterial({ map: paneCanvas.map, transparent: true, depthWrite: false, toneMapped: false, opacity: 0 }));
  pane.renderOrder = 12;
  body.add(pane);

  const query = labelCanvas(1280, 96);
  const queryMesh = new THREE.Mesh(new THREE.PlaneGeometry(3.6, 0.27), new THREE.MeshBasicMaterial({ map: query.map, transparent: true, depthWrite: false, depthTest: false, toneMapped: false, opacity: 0 }));
  queryMesh.renderOrder = 20;
  body.add(queryMesh);
  let queryDrawn = '';

  const accentCss = `#${accent.getHexString()}`;
  const infoCss = `#${info.getHexString()}`;
  const warnCss = `#${warn.getHexString()}`;
  const textCss = `#${text.getHexString()}`;

  // A path that turns, character by character, into the key: backslashes become slashes and ASCII
  // letters fold to lower case, as vfstool normalizes every path.
  function morph(from, to, progress) {
    let out = '';
    const length = Math.max(from.length, to.length);
    for (let i = 0; i < length; i++) {
      const edge = progress * (length + 6) - i;
      out += edge > 0 ? (to[i] || '') : (from[i] || '');
    }
    return out;
  }

  function drawLabel(label, path, highlight) {
    const key = `${path}|${highlight.toFixed(2)}`;
    if (label.drawn === key) return;
    label.drawn = key;
    const { g, canvas: c } = label;
    g.clearRect(0, 0, c.width, c.height);
    const source = SOURCES[label.source];
    g.textBaseline = 'middle';
    g.font = `700 50px ${mono}`;
    g.fillStyle = source.kind === 'archive' ? warnCss : accentCss;
    g.fillText(String(label.source), 6, 52);
    g.fillStyle = textCss;
    g.fillText(source.name, 60, 52);
    if (label.winner) {
      const w = g.measureText(source.name).width;
      g.font = `700 36px ${mono}`;
      g.fillStyle = accentCss;
      g.fillText('winner', 60 + w + 26, 54);
    }
    g.font = `500 44px ${mono}`;
    g.fillStyle = highlight > 0.5 ? infoCss : 'rgba(210,214,240,0.82)';
    g.fillText(path, 60, 132);
    label.map.needsUpdate = true;
  }

  function drawQuery(value) {
    if (queryDrawn === value) return;
    queryDrawn = value;
    const { g, canvas: c } = query;
    g.clearRect(0, 0, c.width, c.height);
    g.textBaseline = 'middle';
    g.font = `600 44px ${mono}`;
    g.fillStyle = 'rgba(210,214,240,0.7)';
    g.fillText('$', 6, 50);
    g.fillStyle = infoCss;
    g.fillText(value, 40, 50);
    query.map.needsUpdate = true;
  }

  // Post-processing.
  const postScene = new THREE.Scene();
  const postCamera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);
  const postQuad = new THREE.Mesh(quad);
  postQuad.frustumCulled = false;
  postScene.add(postQuad);
  const brightMaterial = fullscreenMaterial(BRIGHT_FRAGMENT, { tInput: { value: sceneTarget.texture }, uThreshold: { value: 1.15 } });
  const blurMaterial = fullscreenMaterial(BLUR_FRAGMENT, { tInput: { value: null }, uDirection: { value: new THREE.Vector2() } });
  const copyMaterial = fullscreenMaterial(/* glsl */ `
    uniform sampler2D tInput;
    varying vec2 vUv;
    void main() { gl_FragColor = texture2D(tInput, vUv); }
  `, { tInput: { value: null } });
  const compositeMaterial = fullscreenMaterial(COMPOSITE_FRAGMENT, {
    tScene: { value: sceneTarget.texture },
    tBloomNear: { value: bloomTargets[0].texture },
    tBloomFar: { value: bloomTargets[2].texture },
    uScrim: { value: new THREE.Vector4(0, 0, 0, 0) },
    uResolution: { value: new THREE.Vector2(1, 1) },
    uTime: { value: 0 },
  });
  function pass(material, destination) {
    postQuad.material = material;
    renderer.setRenderTarget(destination);
    renderer.render(postScene, postCamera);
  }
  function blur(source, scratch, radius) {
    blurMaterial.uniforms.tInput.value = source.texture;
    blurMaterial.uniforms.uDirection.value.set(radius / source.width, 0);
    pass(blurMaterial, scratch);
    blurMaterial.uniforms.tInput.value = scratch.texture;
    blurMaterial.uniforms.uDirection.value.set(0, radius / source.height);
    pass(blurMaterial, source);
  }

  // Layout.
  const quality = { level: 1, slow: 0 };
  let viewWidth = 1;
  let viewHeight = 1;
  let scale = 1;
  let place = { x: 0, y: 0, size: 0, width: 0, height: 0, above: false, text: [0, 0, 0, 0] };
  // How much `explain`'s pane shrinks to fit a narrow screen.
  let fitExplain = 1;
  const anchor = new THREE.Vector3();
  const raycaster = new THREE.Raycaster();
  const plane = new THREE.Plane(new THREE.Vector3(0, 0, 1), 0);
  const ndc = new THREE.Vector2();
  const tmp = new THREE.Vector3();

  // The still stands where the stack will: fitted to the same region, then grown by its margin.
  function placeStill() {
    const spot = placement(root);
    const stackWidth = Math.min(spot.width * 0.94, spot.height * 0.94 * STACK_ASPECT);
    const stillWidth = stackWidth / STILL.stackWidth;
    const stillHeight = stillWidth / STILL.aspect;
    Object.assign(still.style, {
      left: `${spot.x - stillWidth / 2}px`,
      top: `${spot.y - stillHeight / 2}px`,
      width: `${stillWidth}px`,
      height: `${stillHeight}px`,
    });
    root.classList.add('is-placed');
  }

  // The stack's bounds on screen, in the art's pixels: its middle, width and height.
  const corners = [];
  for (const y of [slabBase(0) - ARCHIVE_HEIGHT, slabBase(SOURCES.length - 1) + TILE_HEIGHT]) {
    for (const x of [-1, 1]) for (const z of [-1, 1]) corners.push(new THREE.Vector3(x * width / 2, y, z * depth / 2));
  }
  function projectedBounds() {
    stack.updateMatrixWorld();
    let minX = Infinity; let maxX = -Infinity; let minY = Infinity; let maxY = -Infinity;
    for (const corner of corners) {
      const point = body.localToWorld(corner.clone()).project(camera);
      const x = (point.x + 1) / 2 * viewWidth;
      const y = (1 - point.y) / 2 * viewHeight;
      minX = Math.min(minX, x); maxX = Math.max(maxX, x); minY = Math.min(minY, y); maxY = Math.max(maxY, y);
    }
    return { x: (minX + maxX) / 2, y: (minY + maxY) / 2, width: maxX - minX, height: maxY - minY };
  }

  function layout() {
    const rect = root.getBoundingClientRect();
    viewWidth = Math.max(1, Math.round(rect.width));
    viewHeight = Math.max(1, Math.round(rect.height));
    const dpr = Math.min(window.devicePixelRatio || 1, small ? 1.5 : 1.75) * quality.level;
    renderer.setPixelRatio(dpr);
    renderer.setSize(viewWidth, viewHeight, false);
    const w = Math.max(1, Math.floor(viewWidth * dpr));
    const h = Math.max(1, Math.floor(viewHeight * dpr));
    sceneTarget.setSize(w, h);
    bloomTargets[0].setSize(Math.max(1, w >> 2), Math.max(1, h >> 2));
    bloomTargets[1].setSize(Math.max(1, w >> 2), Math.max(1, h >> 2));
    bloomTargets[2].setSize(Math.max(1, w >> 3), Math.max(1, h >> 3));
    bloomTargets[3].setSize(Math.max(1, w >> 3), Math.max(1, h >> 3));
    camera.aspect = viewWidth / viewHeight;
    camera.updateProjectionMatrix();
    camera.updateMatrixWorld();
    skyUniforms.uResolution.value.set(viewWidth, viewHeight);
    compositeMaterial.uniforms.uResolution.value.set(viewWidth, viewHeight);

    place = placement(root);
    placeStill();
    ndc.set(place.x / viewWidth * 2 - 1, -(place.y / viewHeight * 2 - 1));
    raycaster.setFromCamera(ndc, camera);
    raycaster.ray.intersectPlane(plane, anchor);
    // Fit the stack, as posed at rest, to the free region: measure it on screen at scale 1, then
    // scale it to fit and move it so its middle is the region's.
    stack.rotation.set(PITCH, YAW, 0, 'XYZ');
    stack.scale.setScalar(1);
    stack.position.copy(anchor);
    const unitBounds = projectedBounds();
    const fit = Math.min(place.width * 0.94 / Math.max(1, unitBounds.width), place.height * 0.94 / Math.max(1, unitBounds.height));
    scale = Math.max(0.05, fit);
    stack.scale.setScalar(scale);
    const bounds = projectedBounds();
    ndc.set((place.x - (bounds.x - place.x)) / viewWidth * 2 - 1, -((place.y - (bounds.y - place.y)) / viewHeight * 2 - 1));
    raycaster.setFromCamera(ndc, camera);
    raycaster.ray.intersectPlane(plane, anchor);
    stack.position.copy(anchor);
    stack.updateMatrixWorld();
    fitShadow();
    const nearDistance = camera.position.distanceTo(anchor) - LADDER.z * scale;
    const pixelsPerUnit = viewHeight / (2 * Math.max(0.1, nearDistance) * Math.tan(THREE.MathUtils.degToRad(camera.fov / 2)));
    // The pane is 4 units wide and sits a quarter unit right of the stack's middle.
    fitExplain = Math.min(1, (viewWidth - 24) / (4.6 * scale * pixelsPerUnit), (viewHeight - 20) / (2.72 * scale * pixelsPerUnit));
    glyphUniforms.uCenter.value.copy(anchor);
    glyphUniforms.uScale.value = scale;
    glyphUniforms.uPixel.value = viewHeight * dpr / (2 * Math.tan(THREE.MathUtils.degToRad(camera.fov / 2)));
    skyUniforms.uCenter.value.set(place.x / viewWidth, 1 - place.y / viewHeight);
    skyUniforms.uRadius.value = place.size / viewHeight * 0.6;
    // The scrim, in the composite's pixels (origin bottom left).
    const [x0, y0, x1, y1] = place.text;
    compositeMaterial.uniforms.uScrim.value.set(x0 - 12, viewHeight - y1 - 12, x1 + 12, viewHeight - y0 + 12);
  }

  // The pointer: a lamp over the stack, which leans towards it. A pointer held perfectly still on
  // one tile charges it; the explained key's winner, held long enough, runs `explain`.
  const pointer = { x: 0, y: 0, inside: false, touch: false, down: false, anchorX: 0, anchorY: 0, stillSince: 0, moved: 0 };
  const lean = new THREE.Vector2();
  let presence = 0;
  const lampTarget = new THREE.Vector3();
  const lampPosition = new THREE.Vector3();
  function resetStill(event) {
    pointer.anchorX = event.clientX;
    pointer.anchorY = event.clientY;
    pointer.stillSince = performance.now();
  }
  function onMove(event) {
    pointer.x = event.clientX;
    pointer.y = event.clientY;
    pointer.inside = true;
    pointer.touch = event.pointerType === 'touch';
    pointer.moved = performance.now();
    if (Math.hypot(event.clientX - pointer.anchorX, event.clientY - pointer.anchorY) > STILL_PIXELS) resetStill(event);
  }
  function onDown(event) {
    onMove(event);
    pointer.down = true;
    // A mouse press breaks the stillness; a finger resting on the glass is how a phone holds still.
    resetStill(event);
  }
  function onUp(event) {
    pointer.down = false;
    if (event.pointerType === 'touch') pointer.inside = false;
    else resetStill(event);
  }
  function onLeave() {
    pointer.inside = false;
    pointer.down = false;
  }
  const hero = root.closest('.dw-hero') || root;
  if (!reduceMotion) {
    hero.addEventListener('pointermove', onMove, { passive: true });
    hero.addEventListener('pointerdown', onDown, { passive: true });
    hero.addEventListener('pointerup', onUp, { passive: true });
    hero.addEventListener('pointercancel', onLeave, { passive: true });
    hero.addEventListener('pointerleave', onLeave, { passive: true });
    addEventListener('scroll', () => { pointer.stillSince = performance.now(); }, { passive: true });
  }

  // The loop.
  const clock = new THREE.Clock();
  let time = reduceMotion ? 5.5 : 0;
  let visible = false;
  let running = false;
  let first = true;
  let lost = false;
  let lookup = { index: 0, start: 1.5 };
  let collapseStart = COLLAPSE_EVERY - 6;
  let dwell = 0;
  let holdStart = null;
  let explainStart = -100;
  let cooldown = 0;
  let hovered = -1;
  const matrix = new THREE.Matrix4();
  const quaternion = new THREE.Quaternion();
  const scaleVector = new THREE.Vector3();
  const position = new THREE.Vector3();
  const faceCamera = new THREE.Quaternion();
  const inverse = new THREE.Matrix4();
  const euler = new THREE.Euler();
  const lookupKeys = keys.filter((entry) => entry.from.length > 1 || entry.key.endsWith('.bsa'));

  function collapseAmount() {
    if (reduceMotion) return 0;
    const t = time - collapseStart;
    if (t < 0) return 0;
    if (t < 1.8) return smooth(t / 1.8);
    if (t < 4.8) return 1;
    if (t < 6.6) return 1 - smooth((t - 4.8) / 1.8);
    return 0;
  }

  function slabY(index, spread, closed) {
    return THREE.MathUtils.lerp(slabBase(index) * spread + STACK_MIDDLE * (1 - spread), STACK_MIDDLE, closed);
  }

  function frame() {
    running = false;
    if (lost) return;
    const rawDt = clock.getDelta();
    const dt = Math.min(rawDt, 0.1);
    if (!reduceMotion && rawDt < 0.5) {
      quality.slow = rawDt > 1 / 40 ? quality.slow + rawDt : Math.max(0, quality.slow - rawDt * 0.5);
      if (quality.slow > 1.5 && quality.level > 0.5) {
        quality.level = Math.max(0.5, quality.level - 0.2);
        quality.slow = 0;
        layout();
      }
    }
    if (!reduceMotion) time += dt;
    const now = performance.now();
    skyUniforms.uTime.value = time;
    glyphUniforms.uTime.value = time;
    membraneUniforms.uTime.value = time;
    compositeMaterial.uniforms.uTime.value = time;

    // `explain`: 0 before, rising to 1 while the providers stand out, falling back after.
    const explainAge = time - explainStart;
    const explaining = explainAge >= 0 && explainAge < 9;
    const out = explaining ? smooth(explainAge / 1.3) * (1 - smooth((explainAge - 7.2) / 1.6)) : 0;

    // The collapse runs on its own clock, but never while a pointer is being held or `explain` runs.
    if (!reduceMotion && time - collapseStart > COLLAPSE_EVERY) {
      if (dwell > 0.3 || explaining) collapseStart = time - COLLAPSE_EVERY + 4;
      else collapseStart = time;
    }
    const closed = explaining ? 0 : collapseAmount();
    const spread = 1 + out * 0.12;
    dim.value = out;

    // The pose: turned to show the stack's depth, leaning towards the lamp.
    const pointerNdcX = pointer.inside ? ((pointer.x - root.getBoundingClientRect().left) / viewWidth) * 2 - 1 : 0;
    const pointerNdcY = pointer.inside ? -(((pointer.y - root.getBoundingClientRect().top) / viewHeight) * 2 - 1) : 0;
    const idle = !pointer.inside || now - pointer.moved > 5000;
    if (idle) {
      lampTarget.set(anchor.x + Math.sin(time * 0.37) * 1.6 * scale, anchor.y + (0.9 + Math.cos(time * 0.23) * 0.5) * scale, anchor.z + 1.8 * scale);
    } else {
      ndc.set(pointerNdcX, pointerNdcY);
      raycaster.setFromCamera(ndc, camera);
      plane.constant = -(anchor.z + 1.6 * scale);
      if (raycaster.ray.intersectPlane(plane, tmp)) lampTarget.copy(tmp);
      plane.constant = 0;
    }
    presence += ((idle ? 0.4 : 1) - presence) * (reduceMotion ? 1 : Math.min(1, dt * 3));
    lampPosition.lerp(lampTarget, reduceMotion ? 1 : Math.min(1, dt * 5));
    lamp.position.copy(lampPosition);
    lamp.intensity = presence * 12 * scale * scale * (1 - out * 0.9);
    const dx = (lampPosition.x - anchor.x) / scale;
    const dy = (lampPosition.y - anchor.y) / scale;
    const follow = reduceMotion ? 1 : Math.min(1, dt * 3);
    lean.x += (THREE.MathUtils.clamp(-dy * 0.035, -0.07, 0.07) - lean.x) * follow;
    lean.y += (THREE.MathUtils.clamp(dx * 0.04, -0.09, 0.09) - lean.y) * follow;
    // Sway is tiny, so a tile under a still pointer stays under it.
    const sway = reduceMotion ? 0 : 1;
    stack.rotation.set(PITCH + lean.x + Math.sin(time * 0.31) * 0.006 * sway, YAW + lean.y + Math.sin(time * 0.19) * 0.012 * sway, 0, 'XYZ');
    stack.updateMatrixWorld();

    // Slabs.
    for (const slab of slabs) {
      slab.group.position.y = slabY(slab.index, spread, closed);
      const fade = 1 - closed;
      if (slab.archive) {
        const opacity = Math.min(1 - closed * 0.85, 1 - out * 0.7);
        slab.mesh.material.transparent = opacity < 0.999;
        slab.mesh.material.opacity = opacity;
      } else {
        slab.mesh.material.opacity = (slab.index === 3 ? 0.05 : 0.09) * fade * (1 - out * 0.6);
      }
      slab.rim.material.opacity = (slab.index === 3 ? 0.35 : 0.9) * fade * (1 - out * 0.7);
      slab.tag.material.opacity = fade * (1 - out * 0.7);
      slab.group.children.forEach((child) => { if (child.material && child.material.map === slots) child.material.opacity = 0.16 * fade; });
    }
    membrane.position.y = THREE.MathUtils.lerp(slabY(ARCHIVES, spread, 0) - (LOOSE_GAP + SPACING) / 2 * spread + 0.02, STACK_MIDDLE - 0.08, closed);
    membraneUniforms.uOpacity.value = (1 - closed) * (1 - out * 0.6);
    merged.position.y = STACK_MIDDLE - TRAY_HEIGHT / 2;
    merged.material.opacity = closed * 0.24;
    merged.material.emissiveIntensity = closed * 0.16;
    mergedTag.material.opacity = smooth((closed - 0.6) / 0.4);

    // Which tile the pointer is over: raycast against the tiles as they stood last frame.
    hovered = -1;
    if (pointer.inside && !reduceMotion) {
      ndc.set(pointerNdcX, pointerNdcY);
      raycaster.setFromCamera(ndc, camera);
      const hits = raycaster.intersectObject(tileMesh, false);
      if (hits.length) hovered = hits[0].instanceId;
    }

    // The charge: only the explained key's winner, only a pointer that has not moved, not pressed
    // with a mouse, while the stack stands open.
    const stillFor = (now - pointer.stillSince) / 1000;
    const holding = hovered === target && pointer.inside && !explaining && cooldown <= 0 && closed < 0.01
      && (pointer.touch ? pointer.down : !pointer.down) && stillFor > 0.9;
    // The hold is timed by the clock from its start, not by frames, so a slow device needs no
    // longer a hold; letting go unwinds the charge.
    const realDt = Math.min(rawDt, 0.25);
    if (holding) {
      if (holdStart === null) holdStart = now - dwell * 1000;
      dwell = (now - holdStart) / 1000;
    } else {
      holdStart = null;
      dwell = Math.max(0, dwell - realDt * 2.5);
    }
    cooldown = Math.max(0, cooldown - realDt);
    if (dwell >= DWELL) {
      dwell = 0;
      holdStart = null;
      explainStart = time;
      cooldown = 16;
      root.explainCount = (root.explainCount || 0) + 1;
    }
    const chargeFraction = clamp01(dwell / DWELL);

    // Lookups: every few seconds a beam drops down a key's column to its winner.
    if (!reduceMotion && time - lookup.start > 2.6 && !explaining && closed < 0.01) {
      lookup = { index: (lookup.index + 5) % lookupKeys.length, start: time };
    }
    const lookupEntry = lookupKeys[lookup.index % lookupKeys.length];
    const lookupAge = reduceMotion ? FALL + 0.05 : time - lookup.start;

    // Tiles.
    const winnerQuaternion = new THREE.Quaternion();
    stack.getWorldQuaternion(winnerQuaternion);
    faceCamera.copy(winnerQuaternion).invert();
    inverse.copy(body.matrixWorld).invert();
    tiles.forEach((tile, i) => {
      const slabIndex = tile.source;
      let y = slabY(slabIndex, spread, closed) + TILE_HEIGHT / 2 + 0.004;
      const wantLift = i === hovered && !tile.explain ? 0.05 : (i === target ? chargeFraction * 0.05 : 0);
      tile.lift += (wantLift - tile.lift) * Math.min(1, dt * 8);
      y += tile.lift;
      let size = 1;
      if (!tile.winner) size = 1 - smooth(closed * 1.3);
      position.set(tile.home.x, y, tile.home.z);
      quaternion.identity();
      scaleVector.setScalar(Math.max(0.0001, size));

      // The explained key's providers step out in load order, facing the viewer.
      if (tile.explain && out > 0) {
        const order = explainTiles.findIndex((entry) => entry.i === i);
        const count = explainTiles.length;
        const ladderY = ((order - (count - 1) / 2) * LADDER.step + LADDER.lift) * fitExplain;
        const bow = tile.winner ? 0 : smooth((explainAge - 5.2) / 1.0) * 0.45;
        tmp.set(LADDER.x * fitExplain, ladderY, LADDER.z);
        // Ladder positions are in the stack's own frame before its pose, then turned to face front.
        const ladder = tmp.clone().applyQuaternion(faceCamera).add(new THREE.Vector3(0, STACK_MIDDLE, 0));
        position.lerp(ladder, out);
        euler.set(Math.PI / 2 - bow, 0, 0);
        const facing = new THREE.Quaternion().setFromEuler(euler);
        const target3 = faceCamera.clone().multiply(facing);
        quaternion.slerp(target3, out);
        scaleVector.setScalar(THREE.MathUtils.lerp(1, 1.08 * fitExplain, out));
      }
      matrix.compose(position, quaternion, scaleVector);
      tileMesh.setMatrixAt(i, matrix);

      let glow = 0;
      if (tile.winner && lookupEntry && tile.entry === lookupEntry) {
        const landed = lookupAge - FALL;
        if (landed > 0) glow = Math.exp(-landed * 2.2) * 1.1;
      }
      if (i === target) {
        glow += 0.05 + 0.04 * Math.sin(time * 1.25);
        glow += chargeFraction * 0.4;
        if (out > 0) glow += out * (0.28 + 0.12 * Math.sin(explainAge * 2.4));
      }
      if (i === hovered && i !== target) glow += 0.12;
      tile.glow += (glow - tile.glow) * Math.min(1, dt * 10);
      tileData[i * 4 + 2] = tile.glow;
    });
    tileMesh.instanceMatrix.needsUpdate = true;
    tileMesh.computeBoundingSphere();
    tileGeometry.attributes.aTile.needsUpdate = true;

    // The lookup's beam and landing ring.
    if (lookupEntry && !explaining && closed < 0.01) {
      const winnerSource = lookupEntry.from[lookupEntry.from.length - 1];
      const bottomY = slabY(winnerSource, spread, 0) + TILE_HEIGHT + 0.01;
      const topY = slabY(SOURCES.length - 1, spread, 0) + 1.1;
      const fall = clamp01(lookupAge / FALL);
      const head = THREE.MathUtils.lerp(topY, bottomY, fall * fall);
      const length = Math.max(0.02, Math.min(1.1, topY - head));
      beam.position.set(cellX(lookupEntry.col), head + length / 2, cellZ(lookupEntry.row));
      beam.scale.set(1, length, 1);
      beamUniforms.uIntensity.value = reduceMotion ? 0.8 : (1 - smooth((lookupAge - FALL - 0.15) / 0.8)) * 1.6;
      haloUniforms.uIntensity.value = beamUniforms.uIntensity.value;
      landing.position.set(cellX(lookupEntry.col), bottomY + 0.004, cellZ(lookupEntry.row));
      const ringAge = lookupAge - FALL;
      landing.scale.setScalar(0.4 + clamp01(ringAge) * 0.9);
      landingUniforms.uIntensity.value = ringAge > 0 ? (1 - smooth(ringAge / 1.1)) * 1.4 : 0;
    } else {
      beamUniforms.uIntensity.value = 0;
      haloUniforms.uIntensity.value = 0;
      landingUniforms.uIntensity.value = 0;
    }

    // The charge ring around the explained tile.
    const targetTile = tiles[target];
    if (targetTile) {
      charge.position.set(targetTile.home.x, slabY(targetTile.source, spread, closed) + TILE_HEIGHT + 0.012 + targetTile.lift, targetTile.home.z);
      chargeUniforms.uArc.value = chargeFraction;
      chargeUniforms.uIntensity.value = smooth(chargeFraction * 4) * (1 - out);
    }

    // The labels of `explain`, and the query becoming the key.
    for (const [order, label] of labels.entries()) {
      const entry = explainTiles[order];
      const count = explainTiles.length;
      const ladderY = ((order - (count - 1) / 2) * LADDER.step + LADDER.lift) * fitExplain;
      label.mesh.scale.setScalar(fitExplain);
      tmp.set((LADDER.x + LADDER.labelX + 1.45) * fitExplain, ladderY, LADDER.z).applyQuaternion(faceCamera).add(new THREE.Vector3(0, STACK_MIDDLE, 0));
      label.mesh.position.copy(tmp);
      label.mesh.quaternion.copy(faceCamera);
      const appear = explaining ? smooth((explainAge - 1.0 - order * 0.12) / 0.6) * (1 - smooth((explainAge - 7.0) / 0.8)) : 0;
      label.mesh.material.opacity = appear * (entry.tile.winner || explainAge < 5.2 ? 1 : 1 - smooth((explainAge - 5.2) / 1.0) * 0.55);
      label.mesh.visible = appear > 0.001;
      if (label.mesh.visible) {
        const progress = clamp01((explainAge - 2.8 - order * 0.18) / 1.6);
        drawLabel(label, morph(SPELLING[label.source], EXPLAIN_KEY, progress), progress >= 1 ? 1 : 0);
      }
    }
    pane.visible = out > 0.001;
    pane.material.opacity = out;
    if (pane.visible) {
      pane.scale.setScalar(fitExplain);
      tmp.set((LADDER.x + 1.55) * fitExplain, (LADDER.lift + 0.2) * fitExplain, LADDER.z - 0.12).applyQuaternion(faceCamera).add(new THREE.Vector3(0, STACK_MIDDLE, 0));
      pane.position.copy(tmp);
      pane.quaternion.copy(faceCamera);
    }
    const queryAppear = explaining ? smooth((explainAge - 0.6) / 0.6) * (1 - smooth((explainAge - 7.0) / 0.8)) : 0;
    queryMesh.visible = queryAppear > 0.001;
    queryMesh.material.opacity = queryAppear;
    if (queryMesh.visible) {
      queryMesh.scale.setScalar(fitExplain);
      tmp.set((LADDER.x - 0.22 + 1.8) * fitExplain, ((explainTiles.length / 2) * LADDER.step + LADDER.lift + 0.14) * fitExplain, LADDER.z).applyQuaternion(faceCamera).add(new THREE.Vector3(0, STACK_MIDDLE, 0));
      queryMesh.position.copy(tmp);
      queryMesh.quaternion.copy(faceCamera);
      drawQuery(morph(EXPLAIN_QUERY, `vfstool explain ${EXPLAIN_KEY}`, clamp01((explainAge - 2.4) / 1.4)));
    }

    // The scene, then the bloom and the composite.
    renderer.setRenderTarget(sceneTarget);
    renderer.setClearColor(0x000000, 1);
    renderer.clear();
    renderer.render(scene, camera);

    pass(brightMaterial, bloomTargets[0]);
    blur(bloomTargets[0], bloomTargets[1], 1.0);
    blur(bloomTargets[0], bloomTargets[1], 2.0);
    copyMaterial.uniforms.tInput.value = bloomTargets[0].texture;
    pass(copyMaterial, bloomTargets[2]);
    blur(bloomTargets[2], bloomTargets[3], 1.5);
    blur(bloomTargets[2], bloomTargets[3], 3.0);
    pass(compositeMaterial, null);

    if (first) {
      first = false;
      root.classList.add('is-live');
    }
    if (visible && !reduceMotion && !document.hidden) requestFrame();
  }

  function requestFrame() {
    if (running || lost) return;
    running = true;
    requestAnimationFrame(frame);
  }

  canvas.addEventListener('webglcontextlost', (event) => {
    event.preventDefault();
    lost = true;
    root.classList.remove('is-live');
  });
  canvas.addEventListener('webglcontextrestored', () => {
    canvas.remove();
    still.remove();
    root.classList.remove('is-live', 'is-placed');
    mount(root);
  });

  // Where the explained key's winner stands on screen, for tests that hold a pointer on it, and
  // the stack's own bounds, for rendering the still.
  root.stackBounds = () => projectedBounds();
  root.explainCount = 0;
  root.explainTarget = () => {
    const tile = tiles[target];
    const point = new THREE.Vector3(tile.home.x, slabY(tile.source, 1, 0) + TILE_HEIGHT, tile.home.z);
    body.localToWorld(point).project(camera);
    const rect = root.getBoundingClientRect();
    return { x: rect.left + (point.x + 1) / 2 * rect.width, y: rect.top + (1 - point.y) / 2 * rect.height };
  };

  layout();
  new ResizeObserver(() => {
    layout();
    requestFrame();
  }).observe(root);
  if (document.fonts) {
    document.fonts.ready.then(() => {
      layout();
      requestFrame();
    });
  }
  new IntersectionObserver((entries) => {
    visible = entries.some((entry) => entry.isIntersecting);
    if (visible) {
      clock.getDelta();
      requestFrame();
    }
  }).observe(root);
  document.addEventListener('visibilitychange', () => {
    if (!document.hidden && visible) {
      clock.getDelta();
      requestFrame();
    }
  });
}

for (const root of document.querySelectorAll('[data-dw-hero-art]')) mount(root);
