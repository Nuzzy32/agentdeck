# Third-party notices

agentdeck includes material from the following projects. Their licenses apply to that material only.

## herdr (Apache License 2.0)

- Source: https://github.com/herdrdev/herdr, commit `6b526626f66e22c08962a7f39a83fc9ac2ebda62` (2026-10-23).
- License: Apache License, Version 2.0, full text in `src-tauri/herdr/LICENSE` (shipped in the app under
  `Contents/Resources/licenses/herdr/LICENSE`). herdr has no NOTICE file and names no copyright holder.
- What is used (D-035):
  - `src-tauri/herdr/manifests/*.toml`: the agent-state rule manifests from `src/detect/manifests/`, copied unchanged
    except for a header stating their origin.
  - `src-tauri/src/detect.rs`: a modified port of the rule engine in `src/detect/manifest.rs` (schema, validation,
    matching, regions) and of the working → idle hold in `src/pane/agent_detection.rs`. Changes are listed in the
    file's header.
