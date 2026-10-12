# /// script
# requires-python = ">=3.11"
# dependencies = ["pymupdf==1.28.2"]
# ///
"""Exercise the real CLI and a PDF reader, including ActualText and Type3 glyphs.

Run after building Moli:
    uv run scripts/check-native-pdf.py --moli target/debug/moli
Unlike lopdf's basic extractor, MuPDF interprets ActualText, reversed text and
colored Type3 glyphs. Keep these reader checks separate from the Rust structural
and plain-text regressions. Artifacts and observed process costs are retained.
"""

import argparse
import base64
import json
from pathlib import Path
import subprocess
import time

import pymupdf

ROOT = Path(__file__).resolve().parents[1]


def data_url(data, media_type):
    return f"data:{media_type};base64," + base64.b64encode(data).decode()


def capture(binary, output, name, html):
    path = output / f"{name}.pdf"
    started = time.perf_counter()
    peak_kib = 0
    with path.open("wb") as target:
        process = subprocess.Popen(
            [str(binary), "fetch", "--font", "--layout", "--wait-until", "load",
             "--log-level", "error", "--http-no-proxy", "*", "--dump", "pdf",
             data_url(html.encode(), "text/html")],
            stdout=target, stderr=subprocess.PIPE,
        )
        while process.poll() is None:
            if time.perf_counter() - started > 120:
                process.kill()
                process.communicate()
                raise TimeoutError(f"{name} exceeded 120 seconds")
            try:
                for line in Path(f"/proc/{process.pid}/status").read_text().splitlines():
                    if line.startswith("VmHWM:"):
                        peak_kib = max(peak_kib, int(line.split()[1]))
            except FileNotFoundError:
                pass
            time.sleep(0.005)
        stderr = process.communicate()[1].decode(errors="replace")
    if process.returncode:
        raise RuntimeError(f"{name} failed: {stderr}")
    seconds = time.perf_counter() - started
    document = pymupdf.open(path)
    text = " ".join(" ".join(page.get_text().split()) for page in document)
    (output / f"{name}.txt").write_text(text)
    document[0].get_pixmap(matrix=pymupdf.Matrix(1.5, 1.5)).save(output / f"{name}.png")
    info = {"pages": len(document), "pdf_bytes": path.stat().st_size,
            "seconds": round(seconds, 4), "peak_rss_kib_sampled": peak_kib,
            "fonts": sorted({font[3] for page in document for font in page.get_fonts()}),
            "image_count": sum(len(page.get_images()) for page in document)}
    return text, info


def check_button_backgrounds(binary, output, html):
    screenshot = output / "backgrounds-screenshot.png"
    result = subprocess.run(
        [str(binary), "fetch", "--font", "--layout", "--wait-until", "load",
         "--log-level", "error", "--http-no-proxy", "*", "--dump", "screenshot",
         data_url(html.encode(), "text/html")],
        capture_output=True, check=True, timeout=120,
    )
    screenshot.write_bytes(result.stdout)
    reference = pymupdf.Pixmap(screenshot)
    with pymupdf.open(output / "backgrounds.pdf") as document:
        page = document[0]
        # CLI PDF fits the viewport width inside the default 1 cm margins.
        margin = 72 / 2.54
        scale = (page.rect.width - 2 * margin) / reference.width
        clip = pymupdf.Rect(margin, margin, page.rect.width - margin,
                            margin + reference.height * scale)
        rendered = page.get_pixmap(matrix=pymupdf.Matrix(1 / scale, 1 / scale), clip=clip)
        rendered.save(output / "backgrounds-rendered.png")
        for x, y in [(110, 112), (320, 112), (460, 112)]:
            expected = reference.pixel(x, y)[:3]
            actual = rendered.pixel(round(margin / scale + x - rendered.x),
                                    round(margin / scale + y - rendered.y))[:3]
            assert min(expected) < 180, ("missing screenshot background", x, y, expected)
            assert max(abs(a - b) for a, b in zip(actual, expected)) <= 3, (
                "PDF button background differs from screenshot", x, y, actual, expected
            )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--moli", type=Path, default=ROOT / "target/debug/moli")
    parser.add_argument("--output-dir", type=Path, default=ROOT / "target/native-pdf-check")
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    font = data_url((ROOT / "moli-paint/tests/fixtures/moli-pdf.ttf").read_bytes(), "font/ttf")
    cjk = data_url((ROOT / "moli-layout/tests/fixtures/moli-cjk.ttf").read_bytes(), "font/ttf")
    style = f"""<meta charset=utf-8><style>
@font-face{{font-family:PDFTest;src:url({font});font-weight:400 900}}
@font-face{{font-family:CJK;src:url({cjk})}}
body{{font:20px PDFTest,CJK;margin:24px}} p{{margin:16px 0}}
</style>"""
    results = {}
    text, results["minimal"] = capture(args.moli.resolve(), args.output_dir, "minimal",
                                        style + "<h1>Font Text 123</h1>")
    assert text == "Font Text 123", text
    assert results["minimal"]["image_count"] == 0
    assert any("+MoliPDFTest" in font for font in results["minimal"]["fonts"])

    html = style + """<style>
button{position:absolute;left:100px;top:100px;width:180px;height:60px;
color:white;background:#325dff;border:0;border-radius:12px;font:20px PDFTest}
#gradient{left:300px;background:linear-gradient(90deg in srgb,#325dff,#9864ff)}
</style><button>Search</button><button id=gradient>Gradient</button>"""
    text, results["backgrounds"] = capture(args.moli.resolve(), args.output_dir, "backgrounds", html)
    assert text == "Search Gradient", text
    check_button_backgrounds(args.moli.resolve(), args.output_dir, html)

    expected = ["Font Text 123 中文", "office fi ffi á", "Second page a", "abc אבג xyz",
                "Color ★", "Weight 650", "Italic"]
    html = style + """<p>Font Text 123 中文</p><p>office fi ffi á</p>
<p>Second page a</p><p>abc אבג xyz</p><p>Color ★</p>
<p style="font-weight:650">Weight 650</p><p style="font-style:italic">Italic</p>"""
    text, results["shaping"] = capture(args.moli.resolve(), args.output_dir, "shaping", html)
    assert text == " ".join(expected), text

    html = style + """<p style="opacity:.65">Opacity text</p>
<p style="text-shadow:2px 2px 2px red">Shadow text once</p>
<p style="filter:blur(.3px)">Filtered text once</p>
<p style="opacity:0">Hidden opacity</p>
<p style="mask-image:linear-gradient(transparent,transparent)">Hidden mask</p>"""
    text, results["effects"] = capture(args.moli.resolve(), args.output_dir, "effects", html)
    for phrase in ["Opacity text", "Shadow text once", "Filtered text once"]:
        assert text.count(phrase) == 1, text
    assert "Hidden" not in text, text

    # Exceeds the full-document screenshot CSS dimension limit. There is no
    # full-document pixel allocation in this path, even at the default viewport.
    html = style + '<main style="height:200100px">' + "".join(
        f'<p style="position:absolute;top:{i * 700}px">Row {i:03}</p>' for i in range(286)
    ) + "</main>"
    text, results["long_document"] = capture(args.moli.resolve(), args.output_dir, "long-document", html)
    assert text == " ".join(f"Row {i:03}" for i in range(286)), text
    assert results["long_document"]["pages"] > 1
    assert results["long_document"]["image_count"] == 0
    report = {"binary": str(args.moli.resolve()), "binary_bytes": args.moli.stat().st_size,
              "reader": pymupdf.version[0], "cases": results}
    (args.output_dir / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
