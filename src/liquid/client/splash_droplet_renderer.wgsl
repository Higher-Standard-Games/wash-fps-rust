// wash-fps liquid/client/SplashDropletRenderer.ts, its GLSL in WGSL (MIT):
// soft, velocity-aligned camera-facing quads of spray. Instances:
//   i0: position.xyz, size   i1: velocity.xyz, opacity

struct VertexOut {
    @builtin(position) clip: vec4f,
    @location(0) corner: vec2f,
    @location(1) opacity: f32,
    @location(2) atm_world: vec3f,
};

@vertex
fn vs_main(in: QuadIn) -> VertexOut {
    let position = in.i0.xyz;
    var centre = globals.view * vec4f(position, 1.0);
    let velocity = (globals.view * vec4f(in.i1.xyz, 0.0)).xy;
    let speed = length(velocity);
    let axis = select(vec2f(0.0, 1.0), velocity / speed, speed > 1.0);
    let side = vec2f(axis.y, -axis.x);
    // The template's uv is already the corner in [-1, 1].
    let corner = in.uv;
    let width = 0.45 * in.i0.w;
    let height = (0.6 + min(speed * 0.003, 1.4)) * in.i0.w;
    centre = vec4f(centre.xy + side * corner.x * width + axis * corner.y * height, centre.zw);
    var out: VertexOut;
    out.clip = globals.projection * centre;
    out.corner = corner;
    // Near-eye specks are suppressed, not blown up into blobs.
    out.opacity = in.i1.w * glsl_smoothstep(4.0, 20.0, centre.z);
    out.atm_world = position;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4f {
    let radius2 = dot(in.corner, in.corner);
    let alpha = (1.0 - glsl_smoothstep(0.05, 1.0, radius2)) * in.opacity * 0.45;
    if alpha < 0.005 { discard; }
    var colour = vec3f(0.58, 0.70, 0.74);
    // World fog, mixed in display space before the linear output.
    let fog = atm_fog_evaluate(globals.fog, in.atm_world, globals.camera.xyz, false);
    colour = mix(colour, fog.rgb, fog.a);
    colour = pow(colour, vec3f(2.2));
    // The batch blends premultiplied.
    return vec4f(colour * alpha, alpha);
}
