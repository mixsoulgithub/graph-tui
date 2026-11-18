# Releasing

Release notes are **generated from commit messages**, not written by hand. One chain, one source:

```text
ordinary commit  ->  CHANGELOG section  ->  release commit body  ->  annotated tag  ->  GitHub Release notes
```

The generated text is byte-identical in the last four places; `scripts/release` writes it once.
There is no handwritten `[Unreleased]` section: if it is missing from the log, it is missing from
the notes.

## Commit format

```text
type(scope): summary

Optional body. Paragraphs are kept as the explanation under this entry.
```

- `scope` is optional (`feat: ...`), and is rendered as a trailing `(scope)`.
- `summary` is imperative and short; the reason goes into the body.
- One commit is one user-visible change. Squash fixups before they land on `main`; never leave
  `wip`, `fixup!`, `update` or `misc` commits there.
- A breaking change is written `change(cli)!: ...` (or `BREAKING CHANGE:` in the body); it lands
  in **Changed** with a `Breaking:` prefix.

| type | in the notes | section |
| --- | --- | --- |
| `feat` | yes | Added |
| `change` | yes | Changed |
| `fix` | yes | Fixed |
| `perf` | yes | Performance |
| `docs` | no | |
| `test` | no | |
| `ci` | no | |
| `chore` | no | |
| `refactor` | no | |
| `build` | no | |
| `style` | no | |

## Cutting a release

1. The tree is clean and you are on `main`.
2. Look at what would be published:

   ```bash
   ./scripts/release 0.18.0 --dry-run
   ```

   It prints the range (`<last v* tag>..HEAD`), the commits it skips and why, and the notes. Two
   runs of the same range print the same text.
3. If a commit is mislabelled or two of them are one change, fix it with `git rebase -i` first.
   Never rewrite a tag that is already published.
4. Cut it:

   ```bash
   ./scripts/release 0.18.0            # local commit + tag only
   ./scripts/release 0.18.0 --push     # push main and the tag: that is what publishes
   ```

   Pushing the tag is the release switch. CI then does the rest (next section).

## What the script does

1. Finds the last `v*` tag (`git describe --tags --abbrev=0 --match 'v*'`) and reads
   `git log <tag>..HEAD --no-merges`.
2. Parses the subjects, skips everything outside the table above and the `release:` commits
   themselves, and renders `### Added` / `### Changed` / `### Fixed` / `### Performance` in that
   order (empty sections are dropped).
3. Checks: clean tree, `main`, the new version is greater than `Cargo.toml`'s, and the tag does not
   exist yet. It refuses when the range has no user-visible change.
4. Bumps `Cargo.toml`, lets `cargo check` update `Cargo.lock`, and rewrites the source version
   mentioned in `docs/format.md` / `docs/format.zh.md`.
5. Inserts the version section and its link line
   (`https://github.com/<owner>/<repo>/releases/tag/vX.Y.Z`) at the top of `CHANGELOG.md`.
6. Commits `release: X.Y.Z` (body = the notes, `--cleanup=verbatim` so `### Added` survives),
   then tags `vX.Y.Z` with the same notes as the annotation.
7. `--push` pushes `main` and the tag; CI creates the GitHub Release and the script retries
   `gh release edit vX.Y.Z --notes-file` for a while. Without `gh`, or without a GitHub `origin`,
   it prints the command to run later.

Nothing reaches crates.io from the script: publishing is the tag's job (see below).

## Options

```text
--dry-run            print everything, change nothing
--range A..B         use this range instead of <last tag>..HEAD (handy for a backfill)
--allow-dirty        do not require a clean tree
--branch NAME        required branch (default main, 'any' to skip the check)
--date YYYY-MM-DD    date in the CHANGELOG heading (default: today)
--slug OWNER/REPO    GitHub slug for the link (default: from `git remote get-url origin`)
--push               push main and the tag, then edit the GitHub Release
```

## What CI does with the tag

`.github/workflows/release.yml` runs on `v*` tags:

- `create-release` makes the GitHub Release, `build` attaches the archives for five targets
  (`graph-tui-<target>.tar.gz` / `.zip`, plus `.sha256`);
- `publish` refuses to run when the tag and `Cargo.toml` disagree, skips the upload when that
  version is already on crates.io, and otherwise runs `cargo publish` with the
  `CARGO_REGISTRY_TOKEN` secret. Without the secret it logs a warning and leaves the crate
  unpublished (a broken token must not turn every release red).

So the order is: `./scripts/release 0.18.0 --push` -> tag leaves the machine -> CI publishes the
crate and builds the archives -> the script fills in the release notes.

## Cadence

A patch release every 2-4 weeks and a minor one every 4-6 weeks is enough; a CI or clippy fix does
not deserve a release of its own -- it waits for the next one.

## Channels

The curl installer, the pre-built archives on the releases page, and the crate. Conda and Homebrew
are no longer published.
