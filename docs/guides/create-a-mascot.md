# Create or recolor a mascot

The supported pack model is declarative: a `familiar.json` manifest plus image
assets under the same pack directory. Packs cannot execute JavaScript or native
code.

## New pack

```powershell
cargo run -p familiar-cli -- pack init my-buddy
# Add original/licensed files under my-buddy/assets and map states in familiar.json.
cargo run -p familiar-cli -- pack validate my-buddy
cargo run -p familiar-cli -- pack build my-buddy
cargo run -p familiar-cli -- pack inspect my-buddy.familiar
```

## Import a local image

Quantize a PNG or JPEG on disk into a pack with a fixed asset path
(`assets/spritesheet.png`). The command will not overwrite an existing `--out`
directory unless you pass `--force`.

```powershell
cargo run -p familiar-cli -- pack import-image --input .\foto.png --out .\my-buddy --colors 64 --layout tiled
cargo run -p familiar-cli -- pack validate .\my-buddy
```

`--layout` is `tiled` (default, one 64 px portrait repeated on a 4×4 sheet),
`sheet` (treat the file as a 256 px contact sheet), or `frame` (a single 64 px
PNG still stored as `assets/spritesheet.png`).

Record author, source, license, and AI assistance in the manifest plus a pack
`NOTICE`. Use only repo-owned or redistributable art. The current desktop bundles
Perrito Tech; installing arbitrary packs through the UI remains future work.

## Perrito Tech palette variants

Perrito Tech keeps one canonical transparent spritesheet. Approved recolors are
exact palette replacements for small state props only, such as the dragging
toy, and are defined in `mascots/perrito-tech/palette-variants.json`. The dog is
kept natural and collar-free in every variant.

```powershell
pnpm assets:variants
pnpm assets:check
pnpm validate:packs
```

`assets:variants` rebuilds teal-derived variants. `assets:check` validates the
canonical 4×4 sheet, runs validator regression tests, and performs a read-only
byte comparison that fails on variant drift. Do not recolor the coat, white
markings, eyes, neck, or transparency, and do not hand-edit generated variants.

## Codex sheet validation

You can check that a local WebP file is a 1536×1872 sheet of at most 20 MiB.
The check uses magic bytes and the image header, not the file extension. PNG
and other formats are rejected. It is validation only: it does not import a
pack, map frames, or claim full compatibility with Codex art tools. Frame
layout is still pending.

```powershell
cargo run -p familiar-cli -- pack validate-codex-sheet .\sheet.webp
```

```python
import subprocess
subprocess.check_call(
    ["cargo", "run", "-p", "familiar-cli", "--", "pack", "validate-codex-sheet", "sheet.webp"]
)
```

A failure names a single reason: unsupported format (only WebP is accepted),
incorrect dimensions (actual and expected sizes), a file larger than 20 MiB
(actual size), or an unreadable file.
