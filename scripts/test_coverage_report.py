#!/usr/bin/env python3
"""Test coverage report for rawshift decoders.

Scans test_data/, test_fixtures/, and tests/*.rs to produce a table showing
which decoders have test images and what test aspects are covered.

Usage:
    python3 scripts/test_coverage_report.py          # terminal table
    python3 scripts/test_coverage_report.py --json   # machine-readable JSON
"""

import argparse
import json
import os
import re
import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parent.parent
TEST_DATA_DIR = PROJECT_ROOT / "test_data"
TEST_FIXTURES_DIR = PROJECT_ROOT / "test_fixtures"
TESTS_DIR = PROJECT_ROOT / "tests"
SRC_FORMATS_DIR = PROJECT_ROOT / "src" / "formats"

# ---------------------------------------------------------------------------
# Decoder inventory (static — update when new decoders are added)
# ---------------------------------------------------------------------------

RAW_DECODERS = [
    {"format": "arw", "label": "ARW",  "impl": "full",      "ext": ["ARW"]},
    {"format": "dng", "label": "DNG",  "impl": "full",      "ext": ["DNG"]},
    {"format": "cr2", "label": "CR2",  "impl": "full",      "ext": ["CR2"]},
    {"format": "cr3", "label": "CR3",  "impl": "meta-only", "ext": ["CR3"]},
    {"format": "crw", "label": "CRW",  "impl": "stub",      "ext": ["CRW"]},
    {"format": "nef", "label": "NEF",  "impl": "full",      "ext": ["NEF"]},
    {"format": "raf", "label": "RAF",  "impl": "full",      "ext": ["RAF"]},
]

STANDARD_DECODERS = [
    {"format": "jpeg", "label": "JPEG", "impl": "full",        "ext": ["jpg", "jpeg"]},
    {"format": "png",  "label": "PNG",  "impl": "full",        "ext": ["png"]},
    {"format": "gif",  "label": "GIF",  "impl": "decode-only", "ext": ["gif"]},
    {"format": "tiff", "label": "TIFF", "impl": "decode-only", "ext": ["tiff", "tif"]},
    {"format": "webp", "label": "WebP", "impl": "full",        "ext": ["webp"]},
    {"format": "svg",  "label": "SVG",  "impl": "gated",       "ext": ["svg"]},
    {"format": "jxl",  "label": "JXL",  "impl": "full",        "ext": ["jxl"]},
    {"format": "avif", "label": "AVIF", "impl": "full",        "ext": ["avif"]},
    {"format": "heic", "label": "HEIC", "impl": "detect-only", "ext": ["heic", "heif"]},
    {"format": "apv",  "label": "APV",  "impl": "detect-only", "ext": ["apv"]},
]

SIDECAR_FILES = ["expected.json", "exiftool.json", "file_identify.txt",
                 "libraw_identify.txt", "dcraw_identify.txt"]

# ---------------------------------------------------------------------------
# Test data scanning
# ---------------------------------------------------------------------------


def count_raw_images(decoder: dict) -> int:
    """Count images in test_data/ matching this RAW decoder's extensions."""
    if not TEST_DATA_DIR.exists():
        return 0
    count = 0
    exts_upper = {e.upper() for e in decoder["ext"]}
    # RAW images live under test_data/<Make>/<Model>/ (not under standard/)
    for root, dirs, files in os.walk(TEST_DATA_DIR):
        # Skip the standard/ subtree for RAW decoders
        dirs[:] = [d for d in dirs if d != "standard"]
        for f in files:
            if Path(f).suffix.lstrip(".").upper() in exts_upper:
                count += 1
    return count


def count_standard_images(decoder: dict) -> int:
    """Count images in test_data/standard/<format>/."""
    fmt_dir = TEST_DATA_DIR / "standard" / decoder["format"]
    if not fmt_dir.exists():
        return 0
    exts = {e.lower() for e in decoder["ext"]}
    return sum(
        1 for f in fmt_dir.iterdir()
        if f.is_file() and f.suffix.lstrip(".").lower() in exts
    )


def check_raw_fixtures(decoder: dict) -> tuple[int, int]:
    """Return (images_with_fixtures, images_total) for a RAW decoder."""
    if not TEST_DATA_DIR.exists() or not TEST_FIXTURES_DIR.exists():
        return 0, 0
    exts_upper = {e.upper() for e in decoder["ext"]}
    total = 0
    with_expected = 0
    for root, dirs, files in os.walk(TEST_DATA_DIR):
        dirs[:] = [d for d in dirs if d != "standard"]
        for f in files:
            if Path(f).suffix.lstrip(".").upper() in exts_upper:
                total += 1
                stem = Path(f).stem
                # fixture dir mirrors test_data path relative to TEST_DATA_DIR
                rel = Path(root).relative_to(TEST_DATA_DIR)
                fixture_dir = TEST_FIXTURES_DIR / rel / stem
                if (fixture_dir / "expected.json").exists():
                    with_expected += 1
    return with_expected, total


def check_standard_fixtures(decoder: dict) -> tuple[int, int]:
    """Return (images_with_fixtures, images_total) for a standard decoder."""
    fmt_dir = TEST_DATA_DIR / "standard" / decoder["format"]
    fixture_dir = TEST_FIXTURES_DIR / "standard" / decoder["format"]
    if not fmt_dir.exists():
        return 0, 0
    exts = {e.lower() for e in decoder["ext"]}
    total = sum(
        1 for f in fmt_dir.iterdir()
        if f.is_file() and f.suffix.lstrip(".").lower() in exts
    )
    has_expected = (fixture_dir / "expected.json").exists()
    with_expected = total if has_expected else 0
    return with_expected, total


# ---------------------------------------------------------------------------
# Test code scanning
# ---------------------------------------------------------------------------


def scan_test_functions() -> dict[str, set[str]]:
    """Scan tests/*.rs and return {format_key: {aspect, ...}}.

    Aspect keys: detect, meta, decode, pipeline, pixels
    """
    if not TESTS_DIR.exists():
        return {}

    # Pattern → (format, aspect)
    patterns = [
        # RAW aspects
        (r"\bfn\s+(\w+)_format_detection\b",     "detect"),
        (r"\bfn\s+(\w+)_metadata_extraction\b",  "meta"),
        (r"\bfn\s+(\w+)_decode_raw",             "decode"),
        (r"\bfn\s+(\w+)_process\b",              "pipeline"),
        # Standard aspects — detect
        (r"\bfn\s+detect_(\w+)_from_file\b",          "detect"),
        (r"\bfn\s+detect_then_decode_(\w+)_from_file\b", "detect"),
        # Standard aspects — decode / pipeline
        (r"\bfn\s+decode_(\w+)_dimensions_from_file\b", "decode"),
        (r"\bfn\s+detect_then_decode_(\w+)_from_file\b", "pipeline"),
        # Standard pixels
        (r"\bfn\s+decode_(\w+)_pixel_values_from_file\b", "pixels"),
        (r"\bfn\s+decode_(\w+)_first_pixel_from_file\b",  "pixels"),
    ]

    result: dict[str, set[str]] = {}

    for rs_file in TESTS_DIR.rglob("*.rs"):
        try:
            src = rs_file.read_text(errors="replace")
        except OSError:
            continue
        for pattern, aspect in patterns:
            for m in re.finditer(pattern, src):
                fmt = m.group(1).lower()
                # Normalize: jpeg_encode → jpeg, gif_decode → gif, etc.
                for suffix in ("_encode", "_decode", "_format"):
                    if fmt.endswith(suffix):
                        fmt = fmt[: -len(suffix)]
                result.setdefault(fmt, set()).add(aspect)

    return result


# ---------------------------------------------------------------------------
# Unit test counting
# ---------------------------------------------------------------------------


def count_unit_tests() -> dict[str, int]:
    """Count #[test] functions per src/formats/<format>.rs file."""
    counts: dict[str, int] = {}
    if not SRC_FORMATS_DIR.exists():
        return counts
    for rs_file in SRC_FORMATS_DIR.glob("*.rs"):
        try:
            src = rs_file.read_text(errors="replace")
        except OSError:
            continue
        n = len(re.findall(r"#\[test\]", src))
        if n > 0:
            counts[rs_file.name] = n
    return counts


def count_all_unit_tests() -> int:
    """Count all #[test] functions across src/."""
    src_dir = PROJECT_ROOT / "src"
    total = 0
    if not src_dir.exists():
        return 0
    for rs_file in src_dir.rglob("*.rs"):
        try:
            src = rs_file.read_text(errors="replace")
        except OSError:
            continue
        total += len(re.findall(r"#\[test\]", src))
    return total


# ---------------------------------------------------------------------------
# Report rendering
# ---------------------------------------------------------------------------


ASPECT_COLS_RAW = ["detect", "meta", "decode", "pipeline"]
ASPECT_COLS_STD = ["detect", "decode", "pipeline", "pixels"]


def aspect_cell(fmt: str, aspect: str, test_fns: dict, image_count: int) -> str:
    """Render a single aspect cell."""
    has_test = aspect in test_fns.get(fmt, set())
    has_data = image_count > 0
    if has_test and has_data:
        return "pass "
    if has_test and not has_data:
        return "skip*"
    return "-    "


def render_table(title: str, decoders: list, count_fn, fixture_fn, test_fns, aspect_cols) -> list[str]:
    lines = [title]
    col_w = 8
    hdr = f"{'Format':<7} {'Impl':<10} {'Images':>6}  {'Fixtures':>8}  " + "  ".join(
        f"{a:<5}" for a in aspect_cols
    )
    sep = "-" * len(hdr)
    lines += [hdr, sep]
    for dec in decoders:
        fmt = dec["format"]
        images = count_fn(dec)
        ok_fixtures, total_fixtures = fixture_fn(dec)
        if total_fixtures > 0:
            fix_str = f"{ok_fixtures}/{total_fixtures} ok"
        else:
            fix_str = "-"
        aspects = "  ".join(
            aspect_cell(fmt, a, test_fns, images) for a in aspect_cols
        )
        lines.append(
            f"{dec['label']:<7} {dec['impl']:<10} {images:>6}  {fix_str:>8}  {aspects}"
        )
    return lines


def render_terminal(test_fns: dict, unit_counts: dict, total_unit: int) -> str:
    lines = ["=== rawshift Test Coverage Report ===", ""]

    # RAW
    raw_lines = render_table(
        "RAW DECODERS",
        RAW_DECODERS,
        count_raw_images,
        check_raw_fixtures,
        test_fns,
        ASPECT_COLS_RAW,
    )
    lines += raw_lines
    lines.append("  * skip = test function exists but no test data (will skip gracefully)")
    lines.append("")

    # Standard
    std_lines = render_table(
        "STANDARD DECODERS",
        STANDARD_DECODERS,
        count_standard_images,
        check_standard_fixtures,
        test_fns,
        ASPECT_COLS_STD,
    )
    lines += std_lines
    lines.append("")

    # Unit tests breakdown
    lines.append("UNIT TESTS (src/formats/*.rs)")
    for fname, count in sorted(unit_counts.items(), key=lambda x: -x[1]):
        lines.append(f"  {fname:<35} {count:>4} tests")
    lines.append("")

    # Summary
    raw_with_data = sum(1 for d in RAW_DECODERS if count_raw_images(d) > 0)
    std_with_data = sum(1 for d in STANDARD_DECODERS if count_standard_images(d) > 0)
    lines.append(
        f"SUMMARY: {raw_with_data}/{len(RAW_DECODERS)} RAW decoders have test data  |  "
        f"{std_with_data}/{len(STANDARD_DECODERS)} standard decoders have test data"
    )
    lines.append(f"         Unit tests: {total_unit} across all src/ files")

    return "\n".join(lines)


def build_json(test_fns: dict, unit_counts: dict, total_unit: int) -> dict:
    raw_rows = []
    for dec in RAW_DECODERS:
        images = count_raw_images(dec)
        ok_fix, total_fix = check_raw_fixtures(dec)
        raw_rows.append({
            "format": dec["format"],
            "label": dec["label"],
            "impl": dec["impl"],
            "images": images,
            "fixtures_ok": ok_fix,
            "fixtures_total": total_fix,
            "tests": {a: (a in test_fns.get(dec["format"], set())) for a in ASPECT_COLS_RAW},
        })

    std_rows = []
    for dec in STANDARD_DECODERS:
        images = count_standard_images(dec)
        ok_fix, total_fix = check_standard_fixtures(dec)
        std_rows.append({
            "format": dec["format"],
            "label": dec["label"],
            "impl": dec["impl"],
            "images": images,
            "fixtures_ok": ok_fix,
            "fixtures_total": total_fix,
            "tests": {a: (a in test_fns.get(dec["format"], set())) for a in ASPECT_COLS_STD},
        })

    return {
        "raw_decoders": raw_rows,
        "standard_decoders": std_rows,
        "unit_tests_by_file": unit_counts,
        "unit_tests_total": total_unit,
    }


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------


def main():
    parser = argparse.ArgumentParser(
        description="Show test coverage report for rawshift decoders."
    )
    parser.add_argument("--json", action="store_true", help="Output machine-readable JSON")
    args = parser.parse_args()

    test_fns = scan_test_functions()
    unit_counts = count_unit_tests()
    total_unit = count_all_unit_tests()

    if args.json:
        print(json.dumps(build_json(test_fns, unit_counts, total_unit), indent=2))
    else:
        print(render_terminal(test_fns, unit_counts, total_unit))


if __name__ == "__main__":
    main()
