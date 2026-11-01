// shader:audit (RUST.md 21.7): which Q3 shader features the shipped maps use.
// Loads every package the way the game does (SHAG's NodePk3 and
// Q3ShaderRegistry), then for each map counts, over the textures its world
// faces draw with, the shader keywords, blend pairs, tcMods, rgbGen and
// alphaGen kinds, deforms and conditionals. Prints JSON.
//
//   node --experimental-transform-types --no-warnings --import <shagger>/tools/golden/ts-register.mjs \
//     tools/shader_audit.mjs <shag> <pk3 dir> > docs/research/shader_audit.json
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const [shagArg, pk3Arg] = process.argv.slice(2);
const load = (path) => import(pathToFileURL(resolve(shagArg, path)).href);
const { ingestPk3Dir } = await load("src/engine/loader/pk3/NodePk3.ts");
const { PK3Loader } = await load("src/engine/loader/pk3/PK3Loader.ts");
const { Q3ShaderRegistry } = await load("src/engine/loader/shader/Q3ShaderRegistry.ts");
const { BspLoader } = await load("src/engine/loader/bsp/BspLoader.ts");
const quiet = () => {};
console.log = console.warn = console.info = console.debug = quiet;

await ingestPk3Dir(resolve(pk3Arg));
const registry = Q3ShaderRegistry.getInstance();
await registry.loadShaders();
const pk3 = PK3Loader.getInstance();

const bump = (counts, key, n = 1) => {
  counts[key] = (counts[key] ?? 0) + n;
};
function audit(shader, faces, c) {
  bump(c.shaders, "with a shader", faces);
  if (shader.cull !== "back") bump(c.shader, `cull ${shader.cull}`, faces);
  for (const p of shader.surfaceParms) bump(c.surfaceparm, p, faces);
  for (const key of ["sort", "polygonOffset", "portal", "nomipmaps", "nopicmip", "noFog", "softParticle", "entityMergable", "fogParms", "skyParms", "glossIntensity", "glossExponent", "surfaceLight"])
    if (shader[key] !== undefined) bump(c.shader, key === "sort" ? `sort ${shader.sort}` : key, faces);
  for (const d of shader.deformVertexes ?? []) bump(c.deform, d.type, faces);
  for (const b of shader.conditionalBlocks ?? []) bump(c.conditional, b.condition, faces);
  bump(c.stageCount, String(shader.stages.length), faces);
  for (const s of shader.stages) {
    const map = s.map.startsWith("$") ? s.map.toLowerCase() : s.animMap ? "animMap" : s.material ? "material" : s.cubeMap ? "cubeMap" : "texture";
    bump(c.stageMap, map, faces);
    bump(c.blend, s.blendFunc ? s.blendFunc.join(" ") : "none (opaque)", faces);
    if (s.alphaFunc) bump(c.stage, `alphaFunc ${s.alphaFunc}`, faces);
    if (s.rgbGen) bump(c.rgbGen, s.rgbGen.type, faces);
    if (s.alphaGen) bump(c.alphaGen, s.alphaGen.type, faces);
    for (const m of s.tcMod ?? []) bump(c.tcMod, m.type, faces);
    if (s.tcGen) bump(c.tcGen, s.tcGen.mode, faces);
    for (const key of ["clampMap", "depthWrite", "depthFunc", "detail"]) if (s[key] !== undefined) bump(c.stage, key === "depthFunc" ? `depthFunc ${s.depthFunc}` : key, faces);
  }
}
const empty = () => ({ faces: 0, textures: 0, shaders: {}, shader: {}, surfaceparm: {}, deform: {}, conditional: {}, stageCount: {}, stageMap: {}, blend: {}, stage: {}, rgbGen: {}, alphaGen: {}, tcMod: {}, tcGen: {} });
const total = empty();
const maps = {};
for (const file of pk3.listFiles("maps/*.bsp").sort()) {
  const bytes = await pk3.getFile(file);
  const bsp = BspLoader.load(bytes);
  const world = bsp.models[0];
  const perTexture = new Map();
  for (let i = world.firstFace; i < world.firstFace + world.numFaces; i++) {
    const t = bsp.faces[i].textureIndex;
    perTexture.set(t, (perTexture.get(t) ?? 0) + 1);
  }
  const c = empty();
  for (const [t, faces] of perTexture) {
    const name = bsp.textures[t]?.name ?? "";
    c.faces += faces;
    c.textures++;
    const shader = registry.getShader(name);
    if (shader) audit(shader, faces, c);
    else bump(c.shaders, "image only", faces);
  }
  const name = file.replace(/^maps\/|\.bsp$/g, "");
  maps[name] = c;
  for (const [k, v] of Object.entries(c)) {
    if (typeof v === "number") total[k] += v;
    else for (const [kk, vv] of Object.entries(v)) bump(total[k], kk, vv);
  }
}
const sorted = (o) => Object.fromEntries(Object.entries(o).sort((a, b) => b[1] - a[1]));
for (const [k, v] of Object.entries(total)) if (typeof v === "object") total[k] = sorted(v);
process.stdout.write(`${JSON.stringify({ tool: "wash-fps-rust tools/shader_audit.mjs", note: "counts are world faces (model 0) drawn with each feature", total, maps }, null, 1)}\n`);
