# Story: Selecting a pgBackRest repository defined on the host [@ANCHOR: backup_management:COMM_story_pgbackrest_repository_selection]

A backup configuration can name a pgBackRest repository that is defined only in `/etc/pgbackrest/pgbackrest.conf` on the host (its storage settings and any cipher
passphrase stay in that file and never travel in the job payload).

## The Process
1. The backup configuration's `pgbackrest_repo` field is read by `_pgbackrest_repo_number()` [@ANCHOR: backup_management:_pgbackrest_repo_number].
2. An absent, empty or zero value means the legacy behaviour: the daemon describes repository 1 itself from the configuration row.
3. A whole number from 1 to 256, or a string of digits, selects that repository; the daemon passes `--repo=N` and sends no storage keys.
4. Anything else (a negative number, a number over 256, text, a list, a boolean or a fractional number) is refused with an error before any command is built.

## Verification
`TestPgbackrestRepoNumber` in `daemon/test_main.py` runs each rule above.
