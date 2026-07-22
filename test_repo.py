import os
import sys
f = "/home/bruce/workspace/hams_open/hams_shared/tools/provision.py"
print("dirname:", os.path.dirname(f))
print("join:", os.path.join(os.path.dirname(f), ".."))
print("abspath:", os.path.abspath(os.path.join(os.path.dirname(f), "..")))
