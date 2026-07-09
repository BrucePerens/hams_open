#!/bin/bash
while true; do
    if python3 hams_shared/tools/test.py -u user_websites_seo; then
        echo "TEST_SUCCESS"
        break
    else
        EXIT_CODE=$?
        if [ $EXIT_CODE -eq 1 ] && grep -q "already running" <<< "$(python3 hams_shared/tools/test.py --help 2>&1 || true)"; then
            # wait and try again
            sleep 10
        else
            echo "TEST_FAILED with $EXIT_CODE"
            break
        fi
    fi
done
