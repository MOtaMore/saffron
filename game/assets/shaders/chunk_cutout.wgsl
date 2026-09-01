// Chunk material extension: fades (via alpha-to-coverage) any fragment that
// sits between the camera and the player and is within `radius` of the player
// on the screen plane. Keeps the player visible under overhangs / trees / roofs.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
}

#ifdef PREPASS_PIPELINE
#import bevy_pbr::{
    prepass_io::{VertexOutput, FragmentOutput},
    pbr_deferred_functions::deferred_output,
}
#else
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}
#endif

struct ChunkCutout {
    player_pos: vec3<f32>,
    radius: f32,
    view_dir: vec3<f32>,
    min_alpha: f32,
    feather: f32,
    enabled: u32,
    _pad0: f32,
    _pad1: f32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100)
var<uniform> cutout: ChunkCutout;

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

#ifdef PREPASS_PIPELINE
    let out = deferred_output(in, pbr_input);
#else
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);

    if (cutout.enabled != 0u) {
        let rel = in.world_position.xyz - cutout.player_pos;
        // Negative = fragment is closer to the camera than the player.
        let along = dot(rel, cutout.view_dir);
        if (along < -0.25) {
            let lateral = length(rel - along * cutout.view_dir);
            let edge = smoothstep(cutout.radius - cutout.feather, cutout.radius, lateral);
            out.color.a = min(out.color.a, mix(cutout.min_alpha, 1.0, edge));
        }
    }
#endif

    return out;
}
