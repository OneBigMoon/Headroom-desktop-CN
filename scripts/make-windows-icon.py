"""Rebuild icon.ico from the app's single logo source via the pinned Tauri CLI."""

import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "src" / "assets" / "headroom-rage-avatar.png"
OUTPUT = ROOT / "src-tauri" / "icons" / "icon.ico"
EXPECTED_SIZES = [16, 24, 32, 48, 64, 256]


def ico_sizes(path: Path) -> list[int]:
    data = path.read_bytes()
    if len(data) < 6:
        raise ValueError("ICO header is truncated")

    reserved, image_type, count = struct.unpack_from("<HHH", data)
    if reserved != 0 or image_type != 1 or len(data) < 6 + count * 16:
        raise ValueError("ICO header is invalid")

    return sorted(data[6 + index * 16] or 256 for index in range(count))


def main() -> None:
    npm = shutil.which("npm.cmd" if os.name == "nt" else "npm")
    if npm is None:
        raise FileNotFoundError("npm is required to run the pinned Tauri icon generator")
    if not SOURCE.is_file():
        raise FileNotFoundError(f"logo source not found: {SOURCE}")

    with tempfile.TemporaryDirectory(prefix="headroom-icons-") as temp_dir:
        subprocess.run(
            [
                npm,
                "run",
                "tauri",
                "--",
                "icon",
                str(SOURCE),
                "--output",
                temp_dir,
            ],
            cwd=ROOT,
            check=True,
        )
        generated = Path(temp_dir) / "icon.ico"
        sizes = ico_sizes(generated)
        if sizes != EXPECTED_SIZES:
            raise ValueError(f"unexpected ICO sizes: {sizes}")
        shutil.copyfile(generated, OUTPUT)

    print(f"wrote {OUTPUT} sizes={EXPECTED_SIZES}")


if __name__ == "__main__":
    main()
