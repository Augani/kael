# Reference Goldens

This directory stores PNG goldens used by `tests/image_diff.rs`.

The checked-in PNG files are deterministic renderer snapshots that keep the
current output stable across refactors. The current representative image-diff
set covers:

- `layer_parenting_basic`
- `polystar_basic`
- `stroke_dash_basic`
- `trim_path_basic`
- `repeater_basic`

`tests/reference/render_fixture.html` remains as the browser-side harness for
future `lottie-web` comparisons, but the checked-in PNG goldens should not be
treated as browser-rendered references unless they are explicitly regenerated
that way.
