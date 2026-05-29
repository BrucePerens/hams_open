# Jules VM Issues - Current Session

## 1. Sibling Repository Accessibility (`hams_community`)
**Issue:** Jules fails to automatically resolve dependencies from `hams_community` when working primarily within the `hams_com` repository.
**Root Cause:** The native Jules workspace initialization (`sudo rm -rf /app ... git clone https://github.com/BrucePerens/hams_com /app`) exclusively clones the target repository. It does not pull sibling repositories into `/app` or the fallback `/hams_community` directory.
**Action Required:** Jules must be explicitly instructed via prompt or bootstrap script to execute `git clone https://github.com/BrucePerens/hams_community /hams_community` (or the equivalent target directory) prior to executing `tools/test.py --provision-jules`.
