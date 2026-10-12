from __future__ import annotations

from dataclasses import dataclass
from io import BytesIO

from pypdf import PdfReader

from .assertions import SmokeError


@dataclass(frozen=True)
class MoliPdfInfo:
    page_count: int
    media_boxes: tuple[tuple[float, float], ...]
    page_text: tuple[str, ...]


def assert_pdf_envelope(data: bytes, label: str) -> None:
    if not data.startswith(b"%PDF-"):
        raise SmokeError(f"{label} did not start with a PDF header")
    if not data.rstrip().endswith(b"%%EOF"):
        raise SmokeError(f"{label} did not end with a PDF EOF marker")


def inspect_moli_pdf(data: bytes, label: str) -> MoliPdfInfo:
    """Inspect actual PDF objects, including compressed objects and Form resources."""
    assert_pdf_envelope(data, label)
    try:
        reader = PdfReader(BytesIO(data), strict=True)
        fonts = []
        visited = set()

        def resolve(value):
            return value.get_object() if hasattr(value, "get_object") else value

        def inspect_resources(resources):
            resources = resolve(resources)
            if id(resources) in visited:
                return
            visited.add(id(resources))
            for ref in resolve(resources.get("/Font", {})).values():
                font = ref.get_object()
                if "/ToUnicode" not in font or not font["/ToUnicode"].get_data():
                    raise SmokeError(f"{label} has text without a Unicode mapping")
                faces = font.get("/DescendantFonts", [font])
                for face in faces:
                    descriptor = face.get_object().get("/FontDescriptor")
                    if descriptor is None or not any(
                        key in descriptor.get_object()
                        and descriptor.get_object()[key].get_data()
                        for key in ("/FontFile", "/FontFile2", "/FontFile3")
                    ):
                        raise SmokeError(f"{label} has a font without embedded data")
                fonts.append(font)
            for ref in resolve(resources.get("/XObject", {})).values():
                obj = ref.get_object()
                if "/Resources" in obj:
                    inspect_resources(obj["/Resources"])

        for page in reader.pages:
            inspect_resources(page.get("/Resources", {}))
        if not fonts:
            raise SmokeError(f"{label} did not contain embedded text fonts")
        return MoliPdfInfo(
            page_count=len(reader.pages),
            media_boxes=tuple(
                (float(page.mediabox.width), float(page.mediabox.height))
                for page in reader.pages
            ),
            page_text=tuple(" ".join(page.extract_text().split()) for page in reader.pages),
        )
    except SmokeError:
        raise
    except Exception as error:
        raise SmokeError(f"{label} was not a readable PDF: {error}") from error
