# Integration backlog

Items the integration role must apply to `docs/design/architecture.md` and the affected packages. Each item
names its evidence. Newest first.

## From the agent toolkit design (2026-10-23)

Evidence: `docs/design/agent_toolkit.md` 13 and 14 (draft, not reviewed).

1. **Core commands as data (architecture M2, testkit scenarios).** Agent tools, the GUI and the scenario DSL
   share one command vocabulary: serde-serialisable command enums with generated JSON Schema, ids stable for the
   life of a document plus path addressing, transactions as one undo step, dry-run on a cloned document, a
   revision counter and structured errors with stable codes. Build this into M2; retrofitting it after M5 is
   expensive.
   **Closed 2026-10-28** by `87b0173` (W-R1.2: command batches as one undo step, `Core::scratch` for dry runs,
   `Refusal::code` with stable `E_` codes, checkpoints) and `eded5f5` (W-R1.3: ids such as `e12` and `g3`, path
   and glob selectors, revision tokens, the 18 tools). The commands were already serde data. The schemas are
   written by hand for the R1 ops (`agent_toolkit_r1.md` 2, 5.3), not generated from `Command`; generated
   schemas (W7.1) and a scenario driver over them come later.
2. **New library crate `cuboid-mcp` (L5) and the `cuboid mcp` subcommand (architecture 3 crate list).** The app
   binary serves MCP over stdio without a window, so `cuboid-cli` stays free of UI and GPU. Decide D-A1 (own
   synchronous subset or `rmcp` with tokio) before W7.2; `deny.toml` bans tokio today.
   **Closed 2026-10-28** by `eded5f5` (the crate, on L6 and not L5: it uses `cuboid-library` on L5, and
   `cuboid-app` moves to L7) and `737950a` (`cuboid mcp --root <folder> [--read-only] [--check]` and its offscreen
   renderer in the app). D-A1 was decided for an own synchronous subset in both protocol eras; the tokio ban
   stays (`agent_toolkit_r1.md` 3, 9). Architecture 3.1 to 3.3 list the crate and the layers with W-R1.6.

## From sample 1 verification (2026-10-18)

Evidence: `docs/spec/formats_native.md` sections 2.1, 4.3.2, 4.5.3, 10.2, 11.1 (all marked "verified (sample 1)").
A private re-encoder reproduced the sample's 953,962-byte BSON payload byte for byte, and, with classic zlib 1.3.1
level 1 in 64 KiB segments, the whole 173,937-byte file.

1. **Texture write policy (architecture 12.3, switch `atlas_recut_argb32`).** When any paint stroke ends, Cubik
   cuts every list texture back out of the packed atlas into an unnamed ARGB32 image without mipmaps and clears
   `n`, `fp` and `cs`. This happens for voxel-only strokes too. Retained texture records therefore go stale at
   the end of every stroke; the policy must model the recut (native 4.5.3) instead of treating it as an edge case.
2. **Container byte identity (architecture 4.1, 12.3).** Neither flate2 backend listed (miniz_oxide, zlib-rs)
   reproduces DotNetZip's output (zlib level 1, 64 KiB parallel segments ending in a sync flush, then `03 00`,
   above 512 KiB). Payload identity (P0) is unaffected. Decide: keep container identity out of scope (default),
   or add an optional classic-zlib backend behind a feature after an owner decision on a C dependency.
3. **Rotation tolerance (architecture 19.1).** A component-wise 1e-4 degree comparison of Euler angles fails at
   gimbal lock (the top camera stores (90, 0, 0); a plain decomposition yields (90, 180, 180)). Compare
   rotations (quaternion distance or rotated basis vectors) instead of Euler components.
4. **Private samples (architecture 19.1, 19.3).** User samples must never enter `tests/golden`. Oracle O1 reads
   them from the folder named by an environment variable (the testkit convention); document the variable and the
   folder layout in the testkit README.
