#!/usr/bin/env python3
"""Require real GPU tests; missing tests/devices/skips are failures."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys


def run(command, log, minimum, required):
    print('Running:', ' '.join(command), flush=True)
    with log.open('w') as output:
        result = subprocess.run(command, stdout=output, stderr=subprocess.STDOUT,
                                text=True, timeout=1200)
    text = log.read_text()
    print(text, end='', flush=True)
    reports = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;', text)
    if result.returncode or len(reports) != 1:
        raise RuntimeError(f'{log}: GPU test command failed or produced no unambiguous result')
    passed, failed, ignored = map(int, reports[0])
    if passed < minimum or failed or ignored:
        raise RuntimeError(f'{log}: required >= {minimum} passes, zero failures and zero skips')
    if 'skipping offscreen test:' in text:
        raise RuntimeError(f'{log}: an offscreen test did not execute on a GPU')
    for name in required:
        # With --nocapture, a test can print evidence between its name and verdict.
        # Stop at the next test header so another test cannot satisfy this verdict.
        verdict = r'test [^\n]*::' + re.escape(name) + r' \.\.\.(?:(?!\ntest ).)*?\bok(?:\r?\n|$)'
        if not re.search(verdict, text, re.DOTALL):
            raise RuntimeError(f'{log}: missing mandatory GPU regression {name}')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--backend', choices=('metal', 'blade', 'directx11', 'webgl2'), required=True)
    parser.add_argument('--evidence', type=Path, required=True)
    args = parser.parse_args()
    args.evidence.mkdir(parents=True, exist_ok=True)
    os.environ.pop('KAEL_HEADLESS', None)
    if args.backend == 'webgl2':
        os.environ['RUSTFLAGS'] = os.environ.get('RUSTFLAGS', '') + ' --cfg getrandom_backend="wasm_js"'
        build = ['cargo', 'test', '--locked', '-p', 'kael', '--lib', '--target', 'wasm32-unknown-unknown',
                 '--no-default-features', '--features', 'browser,custom-shaders', '--no-run', '--message-format=json']
        with (args.evidence / 'browser-build.jsonl').open('w') as output, \
                (args.evidence / 'browser-build.log').open('w') as errors:
            result = subprocess.run(build, stdout=output, stderr=errors, timeout=1200)
        if result.returncode:
            print((args.evidence / 'browser-build.log').read_text(), file=sys.stderr)
            raise RuntimeError('WebGL2 test harness failed to build')
        artifacts = []
        for line in (args.evidence / 'browser-build.jsonl').read_text().splitlines():
            message = json.loads(line)
            if message.get('reason') == 'compiler-artifact' and message['target']['name'] == 'kael' \
                    and message['profile']['test']:
                artifacts.extend(name for name in message['filenames'] if name.endswith('.wasm'))
        if len(artifacts) != 1:
            raise RuntimeError(f'Expected one freshly built browser test artifact, got {artifacts}')
        os.environ['WASM_BINDGEN_TEST_TIMEOUT'] = '60'
        run(['wasm-bindgen-test-runner', artifacts[0],
             'platform::web::renderer::custom_shaders::tests', '--nocapture'],
            args.evidence / 'webgl2-runtime.log', 6,
            ['web_authored_loop_budget_and_exact_pixel_uploads',
             'web_context_loss_invalidates_gpu_handles_and_recovery_has_new_owner',
             'web_all_formats_srgb_scalar_hdr_readback'])
        run(['wasm-bindgen-test-runner', artifacts[0],
             'platform::web::renderer::blur_tests', '--nocapture'],
            args.evidence / 'webgl2-backdrop-blur.log', 8,
            ['web_backdrop_gaussian_keeps_absolute_capture_coordinates',
             'web_backdrop_gaussian_clamps_texel_centers_at_both_viewport_edges',
             'web_backdrop_fractional_capture_includes_last_visible_texel',
             'web_backdrop_preserves_associated_alpha_with_tint_and_saturation',
             'web_backdrop_own_ancestor_masks_and_damage_scissor_restore_scene_state',
             'web_backdrop_lazy_scratch_reuses_compact_capacity_and_rejects_before_allocation',
             'web_backdrop_changed_source_redraws_all_dependent_pixels',
             'web_backdrop_context_loss_discards_generation_and_recovers_pixels'])
        run(['wasm-bindgen-test-runner', artifacts[0],
             'platform::web::renderer::atlas_tests', '--nocapture'],
            args.evidence / 'webgl2-atlas-replay.log', 1,
            ['browser_atlas_gpu_mirror_replay_pressure_retirement_and_reupload_preserve_pixels'])
        print('PROGRAMMABLE_RENDERER_RUNTIME_OK: backend=webgl2', flush=True)
        return
    feature = 'font-kit,runtime_shaders,custom-shaders'
    if args.backend == 'blade':
        feature += ',x11' if sys.platform.startswith('linux') else ',macos-blade'
    command = ['cargo', 'test', '--locked', '-p', 'kael', '--lib', '--no-default-features',
               '--features', feature]
    minimum, compute = {
        'metal': (7, 'native_compute_runtime_buffers_uploads_and_gpu_display'),
        'blade': (5, 'blade_compute_runtime_arrays_uploads_and_loop_budget'),
        'directx11': (11, 'warp_compute_runtime_arrays_exact_uploads_and_guarded_loops'),
    }[args.backend]
    custom_required = [compute]
    if args.backend == 'directx11':
        custom_required.extend(['warp_packed_sprite_filtering_isolates_neighbor_texels_and_preserves_interpolation',
                                'warp_atlas_pressure_retains_replayed_pixels_and_reuploads_after_retirement',
                                'warp_atlas_device_reset_rejects_old_scene_identity_and_rebuilds_pixels',
                                'warp_surviving_atlas_page_reuse_rejects_retired_tile_before_gpu_submission'])
    run(command + ['custom_shaders::tests', '--', '--nocapture', '--test-threads=1'],
        args.evidence / 'custom-shaders-and-compute.log', minimum, custom_required)
    run(command + ['graph_tests', '--', '--nocapture', '--test-threads=1'],
        args.evidence / 'mixed-gpu-graph.log', 6,
        ['mixed_graph_executes_buffer_compute_texture_compute_and_fragment_on_gpu',
         'image_compute_graph_caches_exact_uniforms_and_rejects_invalid_groups_before_allocation'])
    if args.backend == 'metal':
        run(command + ['offscreen_tests', '--', '--nocapture', '--test-threads=1'],
            args.evidence / 'offscreen-scenes.log', 26,
            ['translucent_paths_use_source_over_alpha',
             'backdrop_blur_preserves_premultiplied_color_with_translucent_tint',
             'backdrop_blur_keeps_capture_coordinates',
             'backdrop_blur_clamps_capture_at_texel_centers',
             'backdrop_blur_fractional_capture_includes_last_visible_texel',
             'backdrop_blur_respects_own_and_ancestor_rounded_clips',
             'packed_sprite_filtering_isolates_neighbor_texels_and_preserves_interpolation',
             'atlas_pressure_retains_replayed_pixels_and_reuploads_after_retirement',
             'surviving_atlas_page_reuse_rejects_retired_tile_before_gpu_submission',
             'many_glyph_masks_upload_and_render_every_instance_in_one_batch',
             'native_fractional_glyph_rasters_match_reserved_bounds_and_render_the_complete_line',
             'oversized_readbacks_fail_before_scratch_allocations',
             'offscreen_paths_allocate_on_use_reuse_capacity_and_survive_zero_size',
             'offscreen_gpu_frame_timing_is_opt_in_bounded_and_uses_actual_host_clock'])
        run(command + ['metal_atlas::tests', '--', '--nocapture', '--test-threads=1'],
            args.evidence / 'ordered-atlas-uploads.log', 13,
            ['native_atlas_identity_rejects_foreign_atlas_and_late_release',
             'atlas_texture_uploads_preserve_queued_old_read_then_publish_new_pixels',
             'upload_capacity_rejects_before_raster_retains_pages_and_recovers_with_progress_wake',
             'staging_peak_admission_and_panicking_raster_reservations_roll_back',
             'sixty_four_mib_image_upload_uses_bounded_chunks_and_exact_boundary_pixels',
             'upload_deadline_freezes_retirement_and_retains_resources_without_endless_wakes'])
    elif args.backend == 'blade':
        required = [
            'blade_quad_and_resize_leave_optional_scratch_unallocated',
            'blade_translucent_paths_preserve_source_over_alpha',
            'blade_scratch_pressure_recovery_preserves_owned_gpu_resources_and_pixels',
            'blade_bounded_scene_fence_failures_retain_scratch_and_invalidate_owned_handles',
            'blade_readback_timeout_retains_staging_until_real_fence_completion',
            'blade_backdrop_fractional_capture_includes_last_visible_texel',
            'blade_backdrop_respects_own_and_ancestor_rounded_clips',
            'blade_packed_sprite_filtering_isolates_neighbor_texels_and_preserves_interpolation',
            'blade_atlas_pressure_retains_replayed_pixels_and_reuploads_after_retirement',
            'blade_surviving_atlas_page_reuse_rejects_retired_tile_before_gpu_submission',
        ]
        if sys.platform == 'darwin':
            required.append('blade_scratch_probe_measures_actual_device_allocation_at_4k')
        run(command + ['offscreen_tests', '--', '--nocapture', '--test-threads=1'],
            args.evidence / 'offscreen-scenes.log', len(required), required)
    if args.backend == 'blade':
        run(command + ['blade_atlas::admission_tests', '--', '--nocapture', '--test-threads=1'],
            args.evidence / 'atlas-admission.log', 1,
            ['blade_atlas_admission_accounts_real_upload_buffers_before_raster'])
    run(command + ['native_atlas_identity', '--', '--nocapture', '--test-threads=1'],
        args.evidence / 'atlas-identity-lifetimes.log', 3,
        ['native_atlas_identity_reuses_bounded_slots_without_aliasing_or_late_release',
         'native_atlas_identity_survives_device_list_reset_and_failed_allocations',
         'native_atlas_identity_exhaustion_fails_without_wrapping'])
    run(command + ['atlas_tile_allocations::tests', '--', '--nocapture', '--test-threads=1'],
        args.evidence / 'atlas-tile-residency.log', 3,
        ['retired_tiles_never_alias_after_bucket_generation_wraps',
         'exact_bounds_and_one_time_release_protect_live_allocations',
         'checked_exhaustion_precedes_allocator_mutation_and_rollback_never_recycles'])
    print(f'PROGRAMMABLE_RENDERER_RUNTIME_OK: backend={args.backend}', flush=True)


if __name__ == '__main__':
    main()
