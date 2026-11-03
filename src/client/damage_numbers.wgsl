// wash-fps client/DamageNumbers.ts in WGSL (MIT): a digit of a floating
// damage number, a camera-facing quad (BILLBOARDMODE_ALL) at the place the
// number's canvas texture would put it. Instances:
//   i0: position.xyz, visibility   i1: cell centre.xy, half size.xy (texture px)
//   i2: uv rectangle (u0, v0, u1, v1)   i3: colour.rgb, world units per texture px

@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

struct VertexOut {
    @builtin(position) clip: vec4f,
    @location(0) uv: vec2f,
    @location(1) colour: vec3f,
    @location(2) visibility: f32,
    @location(3) atm_world: vec3f,
};

@vertex
fn vs_main(in: QuadIn) -> VertexOut {
    let corner = in.uv;
    let offset = (in.i1.xy + corner * in.i1.zw) * in.i3.w;
    let world = in.i0.xyz + camera_right() * offset.x + camera_up() * offset.y;
    var out: VertexOut;
    out.clip = globals.view_proj * vec4f(world, 1.0);
    let t = corner * 0.5 + 0.5;
    out.uv = vec2f(mix(in.i2.x, in.i2.z, t.x), mix(in.i2.w, in.i2.y, t.y));
    out.colour = in.i3.rgb;
    out.visibility = in.i0.w;
    out.atm_world = in.i0.xyz;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4f {
    let s = textureSample(atlas, atlas_sampler, in.uv);
    // The fill over the black stroke, as the canvas paints them.
    let fill = s.r;
    let alpha = fill + s.g * (1.0 - fill);
    if alpha * in.visibility < 0.004 { discard; }
    var colour = in.colour * fill / max(alpha, 0.0001);
    // StandardMaterial's fog, mixed in display space before the linear output.
    let fog = atm_fog_evaluate(globals.fog, in.atm_world, globals.camera.xyz, false);
    colour = mix(colour, fog.rgb, fog.a);
    colour = pow(colour, vec3f(2.2));
    let a = alpha * in.visibility;
    // The batch blends premultiplied.
    return vec4f(colour * a, a);
}
