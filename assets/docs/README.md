# The lobby's documents

`TERMS.md` and `CREDITS.md` are copies. The master copies live in wash-fps,
at the root of that repository. Edit them there, then run
`tools/sync_docs.sh` (it reads `../wash-fps` by default) to copy them over.

CI's `docs-sync` job compares these copies with wash-fps `main` and fails,
naming the file, when one is outdated.
