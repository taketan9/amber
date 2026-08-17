// Textured quads drawn over the text layer.
//
// Positions arrive already in clip space, so the vertex stage only passes them
// through: the caller knows the surface size and cian thinks in cells, and
// converting once on the CPU is cheaper than sending a matrix for six vertices.

struct VertexIn {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    // How solid this quad is. A dragged file's ghost is the same picture as the
    // file's icon, drawn faintly — so the opacity travels with the quad rather
    // than with the texture.
    @location(2) alpha: f32,
}

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) alpha: f32,
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    return VertexOut(vec4(in.position, 0.0, 1.0), in.uv, in.alpha);
}

@group(0) @binding(0)
var Texture: texture_2d<f32>;
@group(0) @binding(1)
var Sampler: sampler;

struct Uniforms {
    // 1 when the surface converts linear to sRGB on write, 0 when it does not.
    // Pictures are stored as sRGB bytes either way, so this decides whether
    // they have to be taken back to linear before the hardware re-encodes them.
    use_srgb: u32,
    // Padded out to sixteen bytes by hand, with scalars rather than a `vec3`:
    // a `vec3<u32>` aligns to 16, which would push the struct to 32 and no
    // longer match the `[u32; 4]` sent from Rust.
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(1) @binding(0)
var<uniform> uniforms: Uniforms;

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let color = textureSample(Texture, Sampler, in.uv);
    let factor = select(1.0, 2.2, uniforms.use_srgb == 1u);
    // Alpha is left alone: it is a coverage fraction, not a colour, and
    // gamma-correcting it would eat the edges of every icon.
    return vec4(pow(color.rgb, vec3(factor)), color.a * in.alpha);
}
