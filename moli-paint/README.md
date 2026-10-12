# Paint backends

`raster_snapshot` produces RGBA pixels through AnyRender/Vello CPU.
`pdf::pdf_snapshot` consumes the same owned snapshot and writes PDF through
upstream [Krilla at `5eb4e1f`](https://github.com/LaurenzV/krilla/commit/5eb4e1fb24a24ce78b533c23b080969917530c32).
CDP `Page.printToPDF` and CLI `--dump pdf` use the PDF backend.
The renderer owns the print request, restores media after snapshot capture, and
returns PDF bytes to both clients without publishing interactive geometry. Vector
print capture is independent of the full-document bitmap dimension limit.

The dependency uses a fixed Git revision with only `raster-images` enabled;
`simple-text` remains disabled because Parley already performs shaping. Krilla's
own `pdf-writer` Git pin (`fdf0901`) is recorded in `Cargo.lock`. This revision
requires Rust 1.92; the repository toolchain is 1.96.1. Krilla and the adapter now
share `skrifa` 0.44 with Parley, Vello and usvg. The adapter uses `skrifa::raw`
instead of declaring a separate `read-fonts` dependency. The `subsetter` 0.2.6
dependency still brings `skrifa` 0.42, `read-fonts` 0.39 and `font-types` 0.11:
its upstream `main` also requires those versions, so switching Krilla to Git
does not remove that older stack. Krilla's `imagesize` 0.14 and `base64` 0.22
also coexist with usvg's 0.15 and 0.23; their version requirements do not overlap.

The PDF adapter embeds and subsets fonts, using Parley's existing glyph positions
and the snapshot's shared logical text/UTF-8 cluster ranges. These mappings are
collected only for print demands, keeping them out of the screenshot hot path.
Ligature ranges are joined in logical order before arranging RTL clusters visually. Synthetic italic
uses a glyph-local shear; synthetic weight uses PDF fill-and-stroke. Font instances
are cached separately by font identity and normalized variation coordinates.
The adapter reverses `avar` v1 and `fvar` normalization for Krilla's design-space
variation API. Non-default `avar` v2 instances currently return an explicit error.

Paths, borders, decorations, basic SVG paths, images, sRGB linear and non-repeating
radial gradients, clips, opacity and PDF blend modes use native PDF primitives.
Existing AnyRender geometry lowering is shared for non-text drawing; text bypasses
its glyph API because that API does not carry Unicode semantics. SVG scenes that
cannot be replayed natively, shadows, other gradient interpolation spaces and
filters use bounded CPU raster fallbacks. Ordinary filtered text gets one transparent
text copy for extraction, subject to page ownership, clips and opacity. Fully
transparent groups contribute no text. Curved/rotated clips use a bounded CPU
coverage probe at two samples per CSS pixel to exclude invisible clusters; the
actual glyphs remain native PDF text. Destination masks that leave some visible
ink and contain body text currently return an explicit error: their per-cluster
semantic visibility is not inferred from the result image's bounding box.
Destination-dependent operators (including `plus-lighter`) also require a
containing isolated layer. Shadow and clip-mask glyphs never add semantic copies.
When visible lettering comes only from `background-clip:text` with a transparent
foreground, it is rasterized mask artwork and is not searchable in this version.

`krilla-svg` was evaluated but not added: both its compatible release and the
pinned upstream Git revision use usvg 0.47, while Moli owns usvg 0.48 trees.
Basic SVG geometry reuses the existing AnyRender lowering; more complex SVGs
use a bounded image fallback.

Pagination retains the previous fit-to-printable-width behavior and slices the
continuous print layout. Each text cluster is assigned by the center of its visible, transformed ink
bounds after document clipping. It has one semantic owner page; ink that
crosses onto a neighboring page is outlined there. This prevents off-page runs and
replayed slices from duplicating the extracted document. Device pixel ratio does
not affect PDF page geometry. Page ranges still refer to original document pages.

This does not implement CSS fragmentation (`break-inside`, repeated table headers,
`@page`), tagged PDF, link annotations or document outlines. PDF reading order still
follows paint order, and complex-script extraction depends on the reader's handling
of ToUnicode/ActualText. SVG text that takes a raster fallback has no separate text
mapping. This is not an accessibility or PDF/UA implementation. Krilla
uses ActualText for ambiguous/multiple-glyph clusters and Type3 fonts for some
color glyphs. A reader that ignores these can report incorrect text even when
MuPDF/Poppler extract it correctly; track upstream issues [411](https://github.com/LaurenzV/krilla/issues/411)
and [425](https://github.com/LaurenzV/krilla/issues/425). In the generated shaping
sample, PDF.js 5.6.205 reproduces incorrect extraction of ordinary `a` after the
same glyph was mapped to the `a` + combining acute cluster. MuPDF 1.28.2 and
Poppler 25.03.0 extract those strings correctly. These results were rechecked
with the pinned Git revision. This is an observed compatibility
limit, not a claim of cross-reader complex-text conformance. Apple Preview and
interactive browser selection/copy still require manual checks.

Run the PDF regressions with:

```sh
cargo nextest run -p moli-paint -E 'test(pdf::)'
cargo nextest run -p moli-protocol -E 'test(print_to_pdf) | test(domains::page::pdf::)'
```

`tests/fixtures/moli-pdf.ttf` is original MIT/Apache-2.0 test data derived from the
repository's generated Ahem face. It adds `fi`/`ffi` ligatures and a `wght` axis with
a non-linear `avar` map, combining-mark positioning, Hebrew coverage and COLR
color glyphs. Regenerate it with
`uv run scripts/generate-pdf-test-font.py` from the repository root.

Reader-level regression and artifact generation:

```sh
uv run scripts/check-native-pdf.py --moli target/debug/moli
cd moli-cdp-smoke
uv run moli-cdp-smoke --group pdf
```

The reader script checks the real CLI with MuPDF, including mixed RTL, multi-glyph
clusters, repeated base letters, color glyphs, opacity, shadows, filters and a
200,100 CSS-pixel document. It retains PDFs, rendered pages, extracted text and
per-process timings/sampled peak RSS in `target/native-pdf-check/`. The Rust tests
use lopdf for PDF objects and ordinary text; its extractor does not interpret
ActualText and Type3 text correctly, so it is not the complex-text oracle.

Validation on 2026-10-12 (Linux x86_64, Rust 1.96.1): `cargo fmt --all`,
workspace Clippy with all targets/features and warnings denied, and the full
nextest run passed (20,011 tests; 17 skipped). The Python smoke unit suite passed
43 tests. The real PDF smoke group passed with both debug and release binaries.
The visual comparison covered borders, rounded clips, an sRGB gradient, alpha
image, SVG paths, transformed text, opacity, shadow and blur. Interior gradient
samples differed by at most one 8-bit channel value from the raster screenshot.
The Git revision also passed the real CLI/MuPDF checks and CDP PDF smoke group;
its rendered visual sample was pixel-identical to the crates.io baseline.

The release measurements below were taken with crates.io Krilla 0.8.2 before
the Git switch; they have not been remeasured for the pinned revision.
The repository release profile produced a 158,402,744-byte binary (151.06 MiB,
not stripped by the profile). Compared with the previous lockfile, the production
paint dependency closure adds 11 crate versions, plus a bitflags upgrade required
by the test parser. A same-revision baseline binary was not built, so these
measurements do not establish the PDF backend's binary-size delta.

One observed release CLI run with local data URLs and fixed fonts:

| Input | Pages | PDF bytes | Whole CLI time | Sampled process peak RSS |
| --- | ---: | ---: | ---: | ---: |
| Font Text 123 | 1 | 3,290 | 95 ms | 53.3 MiB |
| Shaping / font variants | 1 | 9,119 | 32 ms | 52.9 MiB |
| Opacity / shadow / blur | 1 | 7,070 | 52 ms | 56.1 MiB |
| 286 rows, 200,100 CSS px | 79 | 45,215 | 144 ms | 59.3 MiB |

These include process startup, font loading, layout and encoding. RSS is sampled
from `/proc` every 5 ms, not an allocator-level peak; timings are observations,
not a performance guarantee. The long document contained no Image XObjects, and
all 286 row labels were extracted once. The effects sample contained two local
raster images, with one semantic copy of each visible text run.
