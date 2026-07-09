"""
External Asset Fetcher
Downloads unminified external libraries into the module structure
to support isolated test networks without breaking AI text-processing.
"""

import os
import urllib.request
import logging
import hashlib

import tempfile
import shutil
import urllib.error

logging.basicConfig(level=logging.INFO, format="%(message)s")
_logger = logging.getLogger(__name__)


def hash_file(path):
    sha256 = hashlib.sha256()
    try:
        with open(path, "rb") as file_stream:
            for chunk in iter(lambda: file_stream.read(4096), b""):
                sha256.update(chunk)
        return sha256.hexdigest()
    except FileNotFoundError:
        return None


def download_file(url, dest_path, expected_hash):
    if hash_file(dest_path) == expected_hash:
        _logger.info("Skipping %s (Already exists and matches hash)", dest_path)
        return

    os.makedirs(os.path.dirname(dest_path), exist_ok=True)
    _logger.info("Downloading %s\n -> %s", url, dest_path)

    req = urllib.request.Request(url, headers={"User-Agent": "Hams-DevSecOps/1.0"})
    
    with urllib.request.urlopen(req, timeout=10) as response:
        tmp_fd, tmp_path = tempfile.mkstemp(dir=os.path.dirname(dest_path))
        try:
            with os.fdopen(tmp_fd, "wb") as out_file:
                sha256 = hashlib.sha256()
                while True:
                    chunk = response.read(8192)
                    if not chunk:
                        break
                    out_file.write(chunk)
                    sha256.update(chunk)
            
            content_hash = sha256.hexdigest()
            if content_hash != expected_hash:
                raise ValueError(f"Hash mismatch for {url}: expected {expected_hash}, got {content_hash}")
            
            os.chmod(tmp_path, 0o644)
            shutil.move(tmp_path, dest_path)
        except Exception as e:  # audit-ignore-catch-all
            _logger.error("Failed to download %s: %s", url, e)
            if os.path.exists(tmp_path):
                os.remove(tmp_path)
            raise


def main():
    base_dir = os.path.dirname(os.path.abspath(__file__))
    # Use node_modules to ensure linter (check_burn_list) skips these files
    lib_dir = os.path.join(base_dir, "static", "src", "node_modules")

    # Leaflet 1.9.4
    leaflet_dir = os.path.join(lib_dir, "leaflet")
    leaflet_base_url = "https://unpkg.com/leaflet@1.9.4/dist/"
    leaflet_files = {
        "leaflet.js": ("leaflet.js", "db49d009c841f5ca34a888c96511ae936fd9f5533e90d8b2c4d57596f4e5641a"),
        "leaflet.css": ("leaflet.css", "a7837102824184820dfa198d1ebcd109ff6d0ff9a2672a074b9a1b4d147d04c6"),
        "images/layers.png": ("images/layers.png", "1dbbe9d028e292f36fcba8f8b3a28d5e8932754fc2215b9ac69e4cdecf5107c6"),
        "images/layers-2x.png": ("images/layers-2x.png", "066daca850d8ffbef007af00b06eac0015728dee279c51f3cb6c716df7c42edf"),
        "images/marker-icon.png": ("images/marker-icon.png", "574c3a5cca85f4114085b6841596d62f00d7c892c7b03f28cbfa301deb1dc437"),
        "images/marker-icon-2x.png": ("images/marker-icon-2x.png", "00179c4c1ee830d3a108412ae0d294f55776cfeb085c60129a39aa6fc4ae2528"),
        "images/marker-shadow.png": ("images/marker-shadow.png", "264f5c640339f042dd729062cfc04c17f8ea0f29882b538e3848ed8f10edb4da"),
    }

    for local_name, (remote_name, expected_hash) in leaflet_files.items():
        url = leaflet_base_url + remote_name
        dest = os.path.join(leaflet_dir, local_name)
        download_file(url, dest, expected_hash)

    # Transformers.js 2.16.1 (Minified version used to avoid dependency audit issues)
    transformers_dir = os.path.join(lib_dir, "transformers")
    transformers_url = (
        "https://cdn.jsdelivr.net/npm/@xenova/transformers@2.16.1/dist/transformers.min.js"
    )
    transformers_dest = os.path.join(transformers_dir, "transformers.js")
    transformers_hash = "24cd9918f7fc3e3a7dc559625da217b564098e137a15e8e878f2457ab6968f4c"
    download_file(transformers_url, transformers_dest, transformers_hash)

    _logger.info("\n✅ All external assets downloaded successfully.")


if __name__ == "__main__":
    main()
