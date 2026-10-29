---
title: Submitting the pull request
summary: Fork, branch, rebase, check, push, open the PR with the template; what the owner approves first; what happens next; the patch route without GitHub.
see_also: [concepts/extending, concepts/extending/checks, concepts/extending/provenance]
verified: code only, 2026-10-29
---

# Submitting the pull request

Publishing is outward-facing: it goes under the owner's name in a public repository. The owner approves the pull request text and the scrubbed fixtures before anything is pushed or opened. Show the owner `git diff --stat`, the fixtures, and the filled template, and wait for a yes in the conversation.

## With GitHub access (the owner's account)

```
gh repo fork BrucePerens/hams_open --clone --remote   # or work from your own clone
cd hams_open
git remote add upstream https://github.com/BrucePerens/hams_open.git   # if the fork command did not
git fetch upstream
git checkout -b bench-<instrument> upstream/main
# ... work, commit in small steps ...
git fetch upstream && git rebase upstream/main
cd daemons/bench_mcp && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cd ../.. && git push -u origin bench-<instrument>
cp .github/PULL_REQUEST_TEMPLATE/bench_instrument.md /tmp/pr-body.md   # fill it in, owner approves it
gh pr create --repo BrucePerens/hams_open --base main --head <owner>:bench-<instrument> \
  --title "bench_mcp: <instrument>: <what>" --body-file /tmp/pr-body.md
```

Branch name `bench-<instrument>`. Commit messages say what and why. A draft PR opened early (see [before you code](concepts/extending/before-you-code)) is the same branch; mark it ready when the template is complete.

The template (`.github/PULL_REQUEST_TEMPLATE/bench_instrument.md`) asks for: instrument model and firmware; what was run on real hardware, by whom, how, and what was not; the safety tier of each tool and why; the provenance statement; the list of new tools; the checks you ran with their results; what you scrubbed. Logs and screenshots are optional and scrubbed.

Without `gh`: open `https://github.com/BrucePerens/hams_open/compare/main...<owner>:bench-<instrument>?template=bench_instrument.md` in a browser (a file under `PULL_REQUEST_TEMPLATE/` is not applied automatically).

## What happens next

A maintainer reviews. Answer in the PR thread only; send no comments, issues or mail to third parties (vendors, upstream projects) about this work. Respond to review feedback by adding commits; do not force-push over feedback, rebase only to resolve conflicts, and say so when you do. The maintainer merges; you do not.

## How your agent reports to the owner

At each step say what you did, what you ran, what you did not run, and what you need next. Ask before: first connecting to the hardware; each higher tier; pushing; opening the PR; replying in the thread with anything the owner has not seen. Report review feedback to the owner before acting on it when it changes what runs on their bench.

## Without GitHub access

Produce the change as files the owner can send or apply:

```
git format-patch upstream/main --output-directory /tmp/bench-<instrument>-patches
git bundle create /tmp/bench-<instrument>.bundle upstream/main..bench-<instrument>
```

Give the owner: the patch directory or bundle, the filled-in template as `PR.md`, the exact commands from the section above for them to run later, and a list of what they must run themselves (the ignored hardware test) and approve. Do not send the files anywhere yourself.
