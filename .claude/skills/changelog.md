# Skill: changelog

Update the changelog when bumping the seval version. Use this skill whenever the user wants to: bump the version, prepare a release, update the changelog, add a changelog entry, or do release prep. Trigger on phrases like "bump version", "prepare release", "update changelog", "new release", "version bump", "cut a release".

## Instructions

You are updating `CHANGELOG.md` and `Cargo.toml` to prepare a version bump for seval. The changelog follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) format and the project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

### Step 1: Determine the New Version

1. Read the current version from `Cargo.toml` (the `version` field in `[package]`).
2. If the user specified a version, use that. Otherwise, ask: "What version are we bumping to? Current is `<version>`. Is this a patch, minor, or major bump?"
3. Compute the new version number.

### Step 2: Gather Changes Since Last Release

Collect all changes since the last tagged release:

```bash
# Find the last release tag
git tag --sort=-v:refname | head -5

# All commits since that tag
git log <last-tag>..HEAD --oneline --no-merges

# Summarize changed files
git diff <last-tag>..HEAD --stat
```

Also check for merged PRs since the last release:

```bash
gh pr list --state merged --base main --search "merged:>=$(git log -1 --format=%ci <last-tag> | cut -d' ' -f1)" --json number,title,labels --jq '.[] | "#\(.number) \(.title) [\(.labels | map(.name) | join(", "))]"'
```

### Step 3: Categorize Changes

Read the existing `CHANGELOG.md` to understand the established section structure and style.

Group the changes into Keep a Changelog categories. Only include categories that have entries:

- **Added** — new features, new tools, new commands, new integrations
- **Changed** — changes to existing functionality, dependency updates with user impact
- **Deprecated** — features that will be removed in a future version
- **Removed** — features that were removed
- **Fixed** — bug fixes, security fixes
- **Security** — vulnerability fixes (reference CVE/RUSTSEC IDs when applicable)

Rules for writing entries:
- Each entry is a single line starting with `- `
- Write in past tense, concise, user-facing language
- Link to PRs where relevant: `([#123](https://github.com/Hari-Nagarajan/seval/pull/123))`
- Group related dependency bumps into a single entry rather than listing each one
- Don't include internal-only changes (CI tweaks, gitignore updates) unless they affect users
- Security fixes should reference the advisory ID (e.g., RUSTSEC-XXXX-XXXX)

### Step 4: Present Draft for Review

Show the user the proposed changelog section:

```
## [<new-version>] - <today's date YYYY-MM-DD>

### Added
- ...

### Fixed
- ...
```

Also show the link reference that will be added at the bottom of the file:
```
[<new-version>]: https://github.com/Hari-Nagarajan/seval/compare/v<old-version>...v<new-version>
```

And note the `Cargo.toml` version bump: `<old-version>` -> `<new-version>`.

Ask: "Here's the changelog draft. Review and let me know what to change, or say 'submit' to apply it."

Iterate on changes until the user approves.

### Step 5: Apply Changes

Once approved, apply all changes:

1. **Update `Cargo.toml`**: change the `version` field to the new version.

2. **Update `Cargo.lock`**: run `cargo check` to regenerate the lockfile with the new version.

3. **Update `CHANGELOG.md`**:
   - Insert the new version section immediately after the header block (after the "adheres to Semantic Versioning" line and blank line).
   - Update the previous version's comparison link at the bottom if needed.
   - Add the new version's link reference at the bottom, maintaining the existing format.

4. Show a summary of all files changed.

### Step 6: Commit

Stage and commit the changes:

```bash
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "chore: release v<new-version>"
```

Do NOT push unless the user explicitly asks.

### Important Rules

- **Always read `CHANGELOG.md`** before editing — match the existing style exactly.
- **Always read `Cargo.toml`** to get the current version — never assume it.
- **Use today's date** for the release date in YYYY-MM-DD format.
- **Don't include routine dependency bumps** unless they fix a security issue or have user-facing impact.
- **Don't push or tag** — the CI workflow handles tagging and releasing when it detects the version bump on main.
- **Keep entries concise** — one line per change, no multi-line descriptions.
