// wash-fps client/TeleportEffect.ts's speed-line burst in WGSL (MIT): a fan
// of spikes (the template, in its local XY plane) facing the camera
// (BILLBOARDMODE_ALL), turned about the view axis, StandardMaterial unlit
// in its emissive colour at a stepped alpha, both sides. Instances:
//   i0: centre.xyz, scale   i1: turn, alpha   i2: colour.rgb

struct VertexOut {
    @builtin(position) clip: vec4f,
    @location(0) colour: vec3f,
    @location(1) alpha: f32,
    @location(2) atm_world: vec3f,
};

@vertex
fn vs_main(in: QuadIn) -> VertexOut {
    let c = cos(in.i1.x);
    let s = sin(in.i1.x);
    let p = in.position.xy;
    let turned = vec2f(p.x * c - p.y * s, p.x * s + p.y * c) * in.i0.w;
    let world = in.i0.xyz + camera_right() * turned.x + camera_up() * turned.y;
    var out: VertexOut;
    out.clip = globals.view_proj * vec4f(world, 1.0);
    out.colour = in.i2.rgb;
    out.alpha = in.i1.y;
    out.atm_world = world;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4f {
    var colour = in.colour;
    let fog = atm_fog_evaluate(globals.fog, in.atm_world, globals.camera.xyz, false);
    colour = mix(colour, fog.rgb, fog.a);
    colour = pow(colour, vec3f(2.2));
    // The batch blends premultiplied.
    return vec4f(colour * in.alpha, in.alpha);
}
