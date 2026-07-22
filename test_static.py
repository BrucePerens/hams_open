import sys
import os
sys.path.insert(0, "/home/bruce/workspace/hams_open")
import hams_shared.tools.infrastructure as infra
env_vars = {"REPO_ROOT": "/home/bruce/workspace/hams_open", "HAMS_COM_DIR": "/home/bruce/workspace/hams_com"}
for spec in infra.MANIFEST["static_files"]:
    if "hams_shared" in spec.get("path", ""):
        print("FOUND:", spec)
        src = infra.format_env(spec.get("src", ""), env_vars)
        print("SRC evaluated:", src)
        print("Exists?", os.path.exists(src))
