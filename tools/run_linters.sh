#!/bin/bash
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# Centralized linter execution script. Silent on success.
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMMUNITY_DIR="$(cd "$DIR/../hams_community" 2>/dev/null && pwd || echo "$DIR/../hams_community")"
PRIMARY_DIR="$(cd "$DIR/../hams_private_primary" 2>/dev/null && pwd || echo "$DIR/../hams_private_primary")"
SECONDARY_DIR="$(cd "$DIR/../hams_private_secondary" 2>/dev/null && pwd || echo "$DIR/../hams_private_secondary")"
TERTIARY_DIR="$(cd "$DIR/../hams_private_tertiary" 2>/dev/null && pwd || echo "$DIR/../hams_private_tertiary")"
ADDONS_PATH="/usr/lib/python3/dist-packages/odoo/addons,$DIR,$COMMUNITY_DIR,$PRIMARY_DIR,$SECONDARY_DIR,$TERTIARY_DIR"
VENV_PYTHON="$DIR/.venv/bin/python"

if [ ! -f "$VENV_PYTHON" ] && [ -f "$PRIMARY_DIR/.venv/bin/python" ]; then
VENV_PYTHON="$PRIMARY_DIR/.venv/bin/python"
fi

LINTERS_FAILED=0

if [ ! -f "$VENV_PYTHON" ]; then
bash "$DIR/tools/setup_venv.sh" > /dev/null 2>&1
fi

# 0. Pre-Flight Dependency Checks
TARGET_MODULES="${1:-}"
if [ -z "$TARGET_MODULES" ]; then
    # Auto-discover all local modules if no specific target is provided
    MOD_ARRAY=()
    for d in "$DIR"/*/; do
        if [ -f "${d}__manifest__.py" ]; then
            MOD_ARRAY+=("$(basename "$d")")
        fi
    done
else
    IFS=',' read -ra MOD_ARRAY <<< "$TARGET_MODULES"
fi

if [ ${#MOD_ARRAY[@]} -gt 0 ]; then
    for MOD in "${MOD_ARRAY[@]}"; do
        if [ -f "$DIR/$MOD/__manifest__.py" ]; then
            MOD_PATH="$DIR/$MOD"
        elif [ -f "$COMMUNITY_DIR/$MOD/__manifest__.py" ]; then
            MOD_PATH="$COMMUNITY_DIR/$MOD"
        elif [ -f "$PRIMARY_DIR/$MOD/__manifest__.py" ]; then
            MOD_PATH="$PRIMARY_DIR/$MOD"
        elif [ -f "$SECONDARY_DIR/$MOD/__manifest__.py" ]; then
            MOD_PATH="$SECONDARY_DIR/$MOD"
        elif [ -f "$TERTIARY_DIR/$MOD/__manifest__.py" ]; then
            MOD_PATH="$TERTIARY_DIR/$MOD"
        else
            continue
        fi
        
        OUT="$("$VENV_PYTHON" "$DIR/tools/pre_flight_check.py" -m "$MOD_PATH" --addons-path "$ADDONS_PATH" 2>&1)"
        if [ $? -ne 0 ]; then
            echo "$OUT"
            LINTERS_FAILED=1
        fi
    done
fi

# 1. Flake8
if command -v flake8 >/dev/null 2>&1 || [ -f "$DIR/.venv/bin/flake8" ]; then
    FLAKE8_CMD="flake8"
    [ -f "$DIR/.venv/bin/flake8" ] && FLAKE8_CMD="$DIR/.venv/bin/flake8"
    
    OUT="$("$FLAKE8_CMD" "$DIR" --exclude=venv,env,.venv,__pycache__,node_modules --select=E9,F,E402 --per-file-ignores="__init__.py:F401" 2>&1)"
    if [ $? -ne 0 ]; then
        echo "❌ Flake8 Violations:"
        echo "$OUT"
        LINTERS_FAILED=1
    fi
fi

# 2. Burn List Linter
OUT="$("$VENV_PYTHON" "$DIR/tools/check_burn_list.py" "$DIR" 2>&1)"
if [ $? -ne 0 ]; then
    echo "$OUT"
    LINTERS_FAILED=1
elif [ -n "$OUT" ]; then
    echo "$OUT"
fi

# 3. Semantic Anchors Verification
# Repositories stand alone; we strictly scan the current directory.
# Absent module anchors resolve via docs/modules/ contracts.
OUT="$("$VENV_PYTHON" "$DIR/tools/verify_anchors.py" "$DIR" 2>&1)"
if [ $? -ne 0 ]; then
    echo "$OUT"
    LINTERS_FAILED=1
elif [ -n "$OUT" ]; then
    echo "$OUT"
fi

if [ $LINTERS_FAILED -ne 0 ]; then
    echo "🛑 Halting due to linter violations. Please review the output above."
    exit 1
else
    exit 0
fi
