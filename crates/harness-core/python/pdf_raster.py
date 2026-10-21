"""Renders pages of a PDF to PNG files for OCR (pypdfium2: Apache-2.0, pdfium in the wheel).

Usage: pdf_raster.py <pdf> <out_dir> <dpi> <max_edge_px> <pages>
  pages: comma separated 1-based page numbers, or "count" to only report the page count.
Prints one JSON line: {"ok": true, "page_count": N, "pages": [{"number": n, "file": "..."}]}
or {"ok": false, "error": "<code>", "detail": "..."}.
"""
import json
import os
import sys


def fail(code, detail=""):
    print(json.dumps({"ok": False, "error": code, "detail": str(detail)[:500]}))
    sys.exit(0)


def main():
    pdf_path, out_dir, dpi, max_edge, pages = sys.argv[1:6]
    dpi = float(dpi)
    max_edge = float(max_edge)
    try:
        import pypdfium2 as pdfium
    except Exception as e:  # pragma: no cover - the env check catches this first
        fail("missing_module", e)
    try:
        doc = pdfium.PdfDocument(pdf_path)
    except pdfium.PdfiumError as e:
        fail("open_failed", e)
    count = len(doc)
    if pages == "count":
        print(json.dumps({"ok": True, "page_count": count, "pages": []}))
        return
    wanted = [int(p) for p in pages.split(",") if p]
    out = []
    for number in wanted:
        if number < 1 or number > count:
            fail("page_out_of_range", f"{number} of {count}")
        page = doc[number - 1]
        width, height = page.get_size()
        scale = dpi / 72.0
        longest = max(width, height) * scale
        if longest > max_edge:
            scale *= max_edge / longest
        try:
            image = page.render(scale=scale).to_pil().convert("RGB")
            name = os.path.join(out_dir, f"page-{number}.png")
            image.save(name, format="PNG")
        except Exception as e:
            fail("render_failed", e)
        out.append({"number": number, "file": name})
    print(json.dumps({"ok": True, "page_count": count, "pages": out}))


main()
