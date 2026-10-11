#!/usr/bin/env python3
"""Verify iframe load delivery using the same fixtures as the Rust regressions.

uv run --project moli-benchmark python scripts/iframe-load-chromium-differential.py
"""

from __future__ import annotations

import argparse
import asyncio
import importlib.util
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "moli-benchmark"))
# Reuse the existing CDP browser lifecycle (startup, target creation, teardown).
spec = importlib.util.spec_from_file_location(
    "iframe_load_oracle", ROOT / "scripts/layout-phase4-chromium-differential.py"
)
oracle = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = oracle
spec.loader.exec_module(oracle)
FIXTURES = ROOT / "moli-renderer-v8/tests/fixtures"
EXPECTED = FIXTURES / "iframe-load-delivery.chromium.json"
CASES = [
    *(f"inert-{kind}-{method}" for kind in ("parser", "html")
      for method in ("append", "insert", "replace")),
    "inert-adoption", "disconnected-native", "disconnected-foreign",
    *(f"active-parser-{method}" for method in ("append", "insert", "replace")),
    "active-fragment", "active-cross-window", "reinsert", "remove-in-onload",
    "srcdoc", "pending-replacement", "navigate-in-onload", "url-navigation",
]


async def evaluate(client, session_id, expression):
    result, _ = await oracle.command(client, "Runtime.evaluate", {
        "expression": expression, "returnByValue": True, "awaitPromise": True,
    }, session_id=session_id)
    if result.get("exceptionDetails"):
        raise RuntimeError(result["exceptionDetails"])
    return result["result"].get("value")


async def measure(client, session_id, case, *, record):
    _, messages = await oracle.command(client, "Page.navigate", {
        "url": "data:text/html,<!doctype html><body>iframe load probe</body>",
    }, session_id=session_id)
    await oracle.wait_for_load(client, session_id, messages)
    await evaluate(client, session_id, (FIXTURES / "iframe-load-delivery.js").read_text())
    immediate = await evaluate(
        client, session_id, f"iframeLoadDelivery.start({json.dumps(case.name)})")
    # Wait for the expected document loads, then cross task boundaries so a
    # duplicate queued after the first event is visible in the final trace.
    settled = await evaluate(client, session_id, """
        (async () => {
          await iframeLoadDelivery.done;
          await new Promise(resolve => setTimeout(resolve, 0));
          await new Promise(resolve => setTimeout(resolve, 0));
          return iframeLoadDelivery.snapshot();
        })()
    """)
    if case.name in ("pending-replacement", "navigate-in-onload"):
        body = await evaluate(client, session_id, "iframeLoadDelivery.bodyText()")
        if body != "replacement":
            raise AssertionError(f"{case.name}: replacement Document did not finish: {body!r}")
    actual = {"immediate": immediate, "settled": settled}
    if not record:
        expected = json.loads(EXPECTED.read_text())["cases"][case.name]
        if actual != expected:
            raise AssertionError(f"{case.name}: expected {expected!r}, got {actual!r}")
    return actual


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--chromium", type=Path, default=oracle.DEFAULT_CHROMIUM)
    parser.add_argument("--record", action="store_true")
    args = parser.parse_args()
    oracle.fixture_cases = lambda: (
        oracle.Case(name, 800, 600, "", ()) for name in CASES
    )
    oracle.measure_case = measure
    report = asyncio.run(oracle.run(args.chromium.expanduser().resolve(), record=args.record))
    report.pop("tolerance_css_px")
    if args.record:
        lines = ["{", f'  "product": {json.dumps(report["product"])},',
                 f'  "revision": {json.dumps(report["revision"])},', '  "cases": {']
        items = list(report["cases"].items())
        lines.extend(
            f"    {json.dumps(name)}: {json.dumps(case)}" + ("," if i + 1 < len(items) else "")
            for i, (name, case) in enumerate(items)
        )
        EXPECTED.write_text("\n".join([*lines, "  }", "}", ""]))
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
