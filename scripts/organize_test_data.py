#!/usr/bin/env python3
import os
import shutil
import subprocess
import json
import sys
from pathlib import Path

# Configuration
# Use absolute paths or relative to script execution. 
# Assuming script is run from project root for simplicity in relative path math, 
# but let's resolve relative to the script location to be safe if possible, 
# or just rely on CWD being project root as is typical.
PROJECT_ROOT = Path(__file__).resolve().parent.parent
TEST_DATA_DIR = PROJECT_ROOT / "test_data"
TEST_FIXTURES_DIR = PROJECT_ROOT / "test_fixtures"
EXTENSIONS = {".ARW", ".CR2", ".CR3", ".NEF", ".DNG", ".ORF", ".RW2", ".RAF"}

def sanitize(name):
    """Sanitize directory names."""
    return name.strip().replace(" ", "_").replace("/", "-")

def get_metadata(file_path):
    """Extract Make and Model using exiftool."""
    try:
        cmd = ["exiftool", "-j", "-Make", "-Model", str(file_path)]
        result = subprocess.run(cmd, capture_output=True, text=True, check=True)
        data = json.loads(result.stdout)
        if data:
            make = data[0].get("Make", "Unknown").strip()
            model = data[0].get("Model", "Unknown").strip()
            return make, model
    except Exception as e:
        print(f"Error reading metadata for {file_path}: {e}")
    return "Unknown", "Unknown"

def run_exiftool_dump(src_file, dest_dir):
    """Generate full exiftool dump."""
    dest_file = dest_dir / "exiftool.json"
    cmd = ["exiftool", "-j", "-g", "-struct", str(src_file)]
    with open(dest_file, "w") as f:
        subprocess.run(cmd, stdout=f, check=True)

def run_libraw_identify(src_file, dest_dir):
    """Generate generic libraw identification info."""
    # Try raw-identify
    dest_file = dest_dir / "libraw_identify.txt"
    if shutil.which("raw-identify"):
        cmd = ["raw-identify", "-v", str(src_file)]
        with open(dest_file, "w") as f:
            subprocess.run(cmd, stdout=f, check=False)

def organize_and_generate():
    # 1. Inspect all files in test_data (recursively or flat)
    
    # Gather all raw files currently in test_data (recursively)
    raw_files = []
    if not TEST_DATA_DIR.exists():
        print(f"Directory not found: {TEST_DATA_DIR}")
        return

    for root, _, files in os.walk(TEST_DATA_DIR):
        for f in files:
            if Path(f).suffix.upper() in EXTENSIONS:
                raw_files.append(Path(root) / f)

    for raw_file in raw_files:
        print(f"Processing {raw_file.name}...")
        
        # Get metadata
        make, model = get_metadata(raw_file)
        make_clean = sanitize(make)
        model_clean = sanitize(model)
        
        # Determine target paths
        # Structure: test_data/Make/Model/Filename.ext
        target_dir = TEST_DATA_DIR / make_clean / model_clean
        target_file = target_dir / raw_file.name
        
        # Move file if it's not already there
        if raw_file.resolve() != target_file.resolve():
            print(f"  Moving to {target_dir.relative_to(PROJECT_ROOT)}")
            target_dir.mkdir(parents=True, exist_ok=True)
            shutil.move(raw_file, target_file)
        else:
            print("  Already in correct location")
        
        # Setup Fixtures
        # Structure: test_fixtures/Make/Model/Filename/
        fixture_dir = TEST_FIXTURES_DIR / make_clean / model_clean / raw_file.stem
        fixture_dir.mkdir(parents=True, exist_ok=True)
        
        print(f"  Updating fixtures in {fixture_dir.relative_to(PROJECT_ROOT)}")
        
        # 1. Generate/Update exiftool dump
        run_exiftool_dump(target_file, fixture_dir)
        
        # 2. Generate/Update libraw identify
        run_libraw_identify(target_file, fixture_dir)
        
        # 3. Handle existing source-of-truth JSON from the root of test_fixtures if it exists
        old_fixture_json = TEST_FIXTURES_DIR / f"{raw_file.stem}.json"
        target_expected_json = fixture_dir / "expected.json"
        
        if old_fixture_json.exists():
            print(f"  Moving existing manual fixture {old_fixture_json.name} to {target_expected_json.name}")
            shutil.move(old_fixture_json, target_expected_json)

if __name__ == "__main__":
    organize_and_generate()
    print("Done.")
