#!/bin/bash
while true; do
  OUTPUT=$(uv run python hams_shared/tools/test.py --module hams_open/database_management 2>&1)
  EXIT_CODE=$?
  echo "$OUTPUT"
  if [[ "$OUTPUT" == *"Another instance of test.py is already running"* ]]; then
    echo "Waiting for lock..."
    sleep 5
  else
    exit $EXIT_CODE
  fi
done
