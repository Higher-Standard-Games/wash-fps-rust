// wash-fps liquid/client/LiquidInteractionSystem.ts's ring disc (MIT): an
// unlit disc of radius 1 lying flat, scaled to the ring, its colour and
// opacity both the generated ring mask. Instances:
//   i0: position.xyz, radius   i1: colour.rgb, alpha

struct VertexOut {
    @builtin(position) clip: vec4f,
    @location(0) corner: vec2f,
    @location(1) colour: vec4f,
    @location(2) atm_world: vec3f,
};

// RING_BAND_CENTRE and RING_BAND_WIDTH.
const BAND_CENTRE: f32 = 0.78;
const BAND_WIDTH: f32 = 0.22;

@vertex
fn vs_main(in: QuadIn) -> VertexOut {
    // `rotation.x = PI / 2`: the disc lies in XZ.
    let world = in.i0.xyz + vec3f(in.uv.x, 0.0, in.uv.y) * in.i0.w;
    var out: VertexOut;
    out.clip = globals.view_proj * vec4f(world, 1.0);
    out.corner = in.uv;
    out.colour = in.i1;
    out.atm_world = world;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4f {
    // `ringMask`: a soft band near the rim, nothing past the disc's edge.
    let r = length(in.corner);
    let distance = abs(r - BAND_CENTRE) / BAND_WIDTH;
    let band = select(max(0.0, 1.0 - distance * distance), 0.0, r > 1.0);
    let alpha = band * in.colour.a;
    if alpha < 0.002 { discard; }
    // Unlit: the emissive colour through the mask.
    var colour = in.colour.rgb * band;
    let fog = atm_fog_evaluate(globals.fog, in.atm_world, globals.camera.xyz, false);
    colour = mix(colour, fog.rgb, fog.a);
    colour = pow(colour, vec3f(2.2));
    // The batch blends premultiplied.
    return vec4f(colour * alpha, alpha);
}
