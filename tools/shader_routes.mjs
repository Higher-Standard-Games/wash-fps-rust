// shader:routes (RUST.md 23): which material path each world texture takes
// in the browser. Loads every package as the game does (SHAG's NodePk3 and
// Q3ShaderRegistry), resolves each world texture's passes as BspMeshBuilder's
// loadTextures does, and names the branch each pass ends in (environment
// cube, generated cubemap, glow overlay ShaderMaterial, generated PBR, filter
// blend ShaderMaterial, tcGen environment, alpha modes, fullbright, vertex
// lighting) with the faces and maps that use it. Prints JSON.
//
//   node --experimental-transform-types --no-warnings --import <shagger>/tools/golden/ts-register.mjs \
//     tools/shader_routes.mjs <shag> <pk3 dir> > target/shader_routes.json
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
const files = new Set(pk3.listFiles().map((f) => f.toLowerCase()));
const strip = (p) => p.replace(/\\/g, "/").replace(/^\//, "").replace(/\.(tga|jpe?g|png|webp|ktx2?|dds)$/i, "").toLowerCase();
const exists = (p) => ["tga", "jpg", "jpeg", "png", "webp", "ktx", "ktx2"].some((e) => files.has(`${strip(p)}.${e}`));
const prefixes = (p) => {
  if (!p || p.startsWith("$")) return [];
  const n = p.replace(/\\/g, "/");
  return /^(textures|models)\//i.test(n) ? [n] : [n, `textures/${n}`];
};
const first = (cands) => cands.find(exists) ?? null;
const fallbacks = (name) => {
  const out = [name];
  if (name.includes("_") && !name.includes("/")) {
    const parts = name.split("_");
    out.push(parts.join("/"));
    if (parts.length >= 3) out.push(`${parts[0]}/${parts[1]}/${parts.slice(2).join("_")}`);
    if (parts.length >= 4) out.push(`${parts[0]}/${parts[1]}/${parts[2]}/${parts.slice(3).join("_")}`);
  }
  return out;
};
const diffuseCandidates = (name, props) => {
  const d = props.diffuseTexture;
  const c = [];
  if (props.hasMaterialMetadata) {
    if (d && !d.startsWith("$")) c.push(...prefixes(d));
    else if (!d) for (const f of fallbacks(name)) c.push(...prefixes(f));
    return c;
  }
  if (d && !d.startsWith("$")) c.push(...prefixes(d));
  for (const f of fallbacks(name)) c.push(...prefixes(f));
  return c;
};
const optional = (path, blank, base, suffixes) => {
  if (blank) return null;
  if (path) return first(prefixes(path));
  if (!base) return null;
  return first(suffixes.flatMap((s) => prefixes(`${base}${s}`)));
};

// Q3ShaderMaterialFactory's analyzeShaderPattern, for the glow overlay only
// (the factory imports Babylon).
const additive = (s) => !!s.blendFunc && ((s.blendFunc[0] === "GL_ONE" && s.blendFunc[1] === "GL_ONE") || (s.blendFunc[0] === "GL_SRC_ALPHA" && s.blendFunc[1] === "GL_ONE"));
const alphaBlend = (s) => !!s.blendFunc && s.blendFunc[0] === "GL_SRC_ALPHA" && s.blendFunc[1] === "GL_ONE_MINUS_SRC_ALPHA";
const glowName = (m) => { const l = m.toLowerCase(); return l.includes("_glow") || l.includes("_add") || l.includes("_bloom") || l.endsWith("glow"); };
function analyzeShaderPattern(shader) {
  const st = shader.stages;
  let base = -1, lm = -1, glow = -1, alpha = false, tcmod = false, glowAlpha = false;
  for (let i = 0; i < st.length; i++) {
    const s = st[i];
    if (s.map === "$lightmap") { lm = i; continue; }
    if (s.map === "$whiteimage") continue;
    if (s.tcMod?.length) tcmod = true;
    if (s.alphaFunc || alphaBlend(s)) alpha = true;
    if (base === -1) base = i;
    else if (additive(s) || glowName(s.map)) { glow = i; glowAlpha = alphaBlend(s); }
  }
  const count = st.filter((s) => s.map && s.map !== "$lightmap" && s.map !== "$whiteimage").length;
  let pattern;
  if (base === -1) pattern = "base_only";
  else if (shader.surfaceParms.has("nolightmap") && count === 1) pattern = "nolightmap_emissive";
  else if (lm !== -1 && glow !== -1 && count <= 3) pattern = "glow_overlay";
  else pattern = "other";
  return { pattern, baseStageIndex: base, glowStageIndex: glow, glowIsAlphaBlend: glowAlpha };
}

function route(name, shader, props, layered) {
  if (props.generatedCubemap) return ["generated cubemap"];
  if (props.environmentMap) return [`environment cube (${props.tcGen?.mode === "surround" ? "surround" : "reflect"})`];
  const found = first(diffuseCandidates(name, props));
  const tags = [];
  if (!found) {
    if (props.alphaMode === "add") tags.push("missing image: radial gradient");
    else if (props.diffuseTexture?.includes("$whiteimage")) tags.push("missing image: white");
    else if (props.diffuseTexture?.includes("$blackimage")) tags.push("missing image: black");
    else return ["missing image: not drawn"];
  }
  if (shader && !layered && !props.hasMaterialMetadata && props.resolvedBranch === "default" && !shader.conditionalBlocks?.length) {
    const a = analyzeShaderPattern(shader);
    if (a.pattern === "glow_overlay" && found && first(prefixes(shader.stages[a.glowStageIndex].map)))
      return [`glow overlay ShaderMaterial (${a.glowIsAlphaBlend ? "alpha" : "add"})`];
  }
  const base = found ? strip(found) : "";
  const normal = optional(props.normalTexture, props.normalTextureExplicitlyBlank, base, ["_norm", "_normal", "_bump"]);
  const gloss = optional(props.glossTexture, props.glossTextureExplicitlyBlank, base, ["_gloss", "_spec"]);
  const glow = optional(props.glowTexture, props.glowTextureExplicitlyBlank, "", []);
  if (normal) tags.push("normal map");
  if (gloss) tags.push("gloss map");
  if (glow) tags.push(props.glowStage ? "glow shine pass" : "glow emissive");
  const pbr = normal && gloss && !props.isTransparent && props.alphaMode === "opaque" && props.alphaCutoff === undefined && !props.isSky && !props.isFullbright;
  if (pbr) return ["generated PBR", ...tags];
  if (props.alphaCutoff !== undefined && props.alphaCutoff > 0) tags.push(`alpha test ${props.alphaMode}`);
  else if (props.alphaMode === "multiply") return ["filter blend ShaderMaterial", ...tags];
  else tags.push(props.alphaMode);
  if (props.isFullbright) tags.push("fullbright");
  if (props.hasMaterialMetadata && props.usesVertexRgb && props.isFullbright && props.alphaMode === "opaque") tags.push("vertex lighting plugin");
  if (props.tcGen?.mode === "environment") tags.push("tcGen environment (spherical reflection)");
  if (props.rgbGen?.type === "const") tags.push("rgbGen const");
  else if (props.alphaMode === "add" && props.usesVertexRgb) tags.push("additive vertex colour");
  if (props.needsAnimation) tags.push("animated");
  if (props.autosprite || props.autosprite2) tags.push("autosprite");
  if (props.softParticle) tags.push("soft particle");
  if (props.primaryStage?.rgbGen?.distanceRamp || props.primaryStage?.alphaGen?.distanceRamp) tags.push("distance ramp");
  return tags;
}

const routes = {};
for (const file of pk3.listFiles("maps/*.bsp").sort()) {
  const bsp = BspLoader.load(await pk3.getFile(file));
  const map = file.replace(/^maps\/|\.bsp$/g, "");
  const faces = new Map();
  for (const f of bsp.faces) faces.set(f.textureIndex, (faces.get(f.textureIndex) ?? 0) + 1);
  for (const [t, n] of faces) {
    const name = bsp.textures[t]?.name ?? "";
    if (!name || name.toLowerCase().includes("common/")) continue;
    const shader = registry.getShader(name);
    for (const deluxe of [true]) {
      const passes = registry.resolveMaterialPasses(name, { deluxe, worldSurface: true });
      passes.forEach((props, i) => {
        const key = route(name, shader, props, passes.length > 1).join(", ");
        const r = (routes[key] ??= { faces: 0, textures: {} });
        r.faces += n;
        const id = passes.length > 1 ? `${name} [pass ${i}]` : name;
        (r.textures[id] ??= []).includes(map) || r.textures[id].push(map);
      });
    }
  }
}
process.stdout.write(`${JSON.stringify(Object.fromEntries(Object.entries(routes).sort((a, b) => b[1].faces - a[1].faces)), null, 1)}\n`);
