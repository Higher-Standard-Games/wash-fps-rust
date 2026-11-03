// wash-fps client/CartoonHitEffect.ts in WGSL (MIT): a cartoon hit sprite
// on a camera-facing plane (BILLBOARDMODE_ALL, rotated about the view axis),
// StandardMaterial unlit with the artwork's colour and alpha. Instances:
//   i0: position.xyz, scale (the plane's size)   i1: rotation, visibility
//   i2: uv rectangle (u0, v0, u1, v1)

@group(0) @binding(1) var sprites: texture_2d<f32>;
@group(0) @binding(2) var sprite_sampler: sampler;

struct VertexOut {
    @builtin(position) clip: vec4f,
    @location(0) uv: vec2f,
    @location(1) visibility: f32,
    @location(2) atm_world: vec3f,
};

@vertex
fn vs_main(in: QuadIn) -> VertexOut {
    let corner = in.uv;
    let c = cos(in.i1.x);
    let s = sin(in.i1.x);
    let turned = vec2f(corner.x * c - corner.y * s, corner.x * s + corner.y * c) * (0.5 * in.i0.w);
    let world = in.i0.xyz + camera_right() * turned.x + camera_up() * turned.y;
    var out: VertexOut;
    out.clip = globals.view_proj * vec4f(world, 1.0);
    let t = corner * 0.5 + 0.5;
    out.uv = vec2f(mix(in.i2.x, in.i2.z, t.x), mix(in.i2.w, in.i2.y, t.y));
    out.visibility = in.i1.y;
    out.atm_world = in.i0.xyz;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4f {
    let texel = textureSample(sprites, sprite_sampler, in.uv);
    let a = texel.a * in.visibility;
    if a < 0.004 { discard; }
    var colour = texel.rgb;
    let fog = atm_fog_evaluate(globals.fog, in.atm_world, globals.camera.xyz, false);
    colour = mix(colour, fog.rgb, fog.a);
    colour = pow(colour, vec3f(2.2));
    // The batch blends premultiplied.
    return vec4f(colour * a, a);
}
