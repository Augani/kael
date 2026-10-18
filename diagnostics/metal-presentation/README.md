# Independent Metal presentation controls

These controls diagnose whether the current macOS session can produce positive
`MTLDrawable.presentedTime` values. They do not replace Kael's required native
presentation test. GPU completion, drawable callbacks, AppKit visibility, and a
passing independent control cannot turn a failed Kael gate into success.

Run the pure parser tests from the repository root:

```sh
python3 -m unittest discover -s diagnostics/metal-presentation -p 'test_run_controls.py' -v
```

They execute no native command. On an authorized macOS desktop, the separate
driver compiles the pinned probe and runs MTKView, CAMetalLayer, and transactional
CAMetalLayer variants, with 60 submitted frames per variant:

```sh
python3 diagnostics/metal-presentation/run-controls.py
```

The driver creates a fresh evidence directory under
`target/native-renderer-smoke/macos/presentation-controls`, rejecting overwrite.
It retains every command result and raw log, with no retries. It bounds compile
time to 60 seconds and each native variant to 15 seconds, followed by owned
process-group cleanup when needed. `METAL_CONTROL_EVIDENCE` can choose another
fresh directory.

Diagnostics read selected console/login/optional lock-known flags, display
geometry and power state, independent Metal device fields, and one hypervisor
key. They do not collect raw session identity dictionaries or environment/device
registry dumps. Own command arguments can include checkout paths. An absent
optional lock key means unknown. The driver changes no security settings or
permissions and performs no wake or unlock operation.

The initial proposal had only static/parser review. Later physical validation
ran this exact probe/driver: each of three variants completed 60 GPU frames and
60 callbacks, with 59 positive presentation timestamps. That is separate
physical functional evidence; it does not establish hosted CI presentation,
Kael's selected device, performance, latency, or power results. The probe chooses
the system-default Metal device, which need not be Kael's selected adapter on a
machine with multiple GPUs.

CI runs the parser tests before this diagnostic driver, even after a failed Kael
step, and always uploads the existing macOS evidence tree. Both-zero outcomes
call for runner/session investigation. Positive controls with a failed Kael gate
call for framework investigation in that same setup. Keep the required gate
failed until Kael produces real positive presentation timestamps.
