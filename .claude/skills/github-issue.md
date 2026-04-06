# Skill: github-issue

File a structured GitHub issue (bug report or feature request) for seval interactively from Claude Code.

## When to Use

Trigger when the user wants to file a GitHub issue, report a bug, or request a feature. Keywords: "file issue", "report bug", "feature request", "open issue", "create issue", "github issue".

## Instructions

You are filing a GitHub issue against the seval repository using issue templates. Follow this workflow exactly.

### Step 1: Detect Issue Type and Read the Template

Determine from the user's message whether this is a **bug report** or **feature request**.
- If unclear, ask: "Is this a bug report or a feature request?"

Then read the corresponding issue template to understand the required fields:

- Bug report: `.github/ISSUE_TEMPLATE/bug_report.md`
- Feature request: `.github/ISSUE_TEMPLATE/feature_request.md`

Parse the markdown to extract:
- The frontmatter (`name`, `about`, `labels`, `title`)
- Each `## ` section header and its placeholder/prompt text

This is the source of truth for what fields exist and what they're called. Do not assume or hardcode any field names — always derive them from the template file.

### Step 2: Auto-Gather Context

Before asking the user anything, silently gather environment and repo context:

```bash
# Git context
git log --oneline -5
git status --short
git diff --stat HEAD~1 2>/dev/null

# For bug reports — environment detection
uname -s -r -m                          # OS info
sw_vers 2>/dev/null                     # macOS version
rustc --version 2>/dev/null             # Rust version
cargo metadata --format-version=1 --no-deps 2>/dev/null | jq -r '.packages[] | select(.name=="seval") | .version' 2>/dev/null
git rev-parse --short HEAD              # commit SHA fallback
```

Also read recently changed files to infer the affected component and architecture impact.

### Step 3: Pre-Fill and Present the Form

Using the parsed template fields and gathered context, draft values for ALL fields from the template:

- **Section fields**: draft content based on the user's description, git context, and the section's placeholder text for guidance on what's expected.
- **Environment fields** (bug reports): fill with auto-detected values (OS, Rust version, seval version, provider).
- **Optional fields**: fill if there's enough context, otherwise note "(optional — not enough context to fill)".

For bug reports, check `~/.seval/logs/` for recent relevant log output and include it in the Logs section if applicable.

Present the complete draft to the user in a clean readable format:

```
## Issue Draft: <title>
**Type**: Bug Report / Feature Request
**Labels**: <from template frontmatter>

### <Section Header>
<proposed value>

### <Section Header>
<proposed value>
...
```

Ask the user to review: "Here's the pre-filled issue. Review and let me know what to change, or say 'submit' to file it."

If the user requests changes, update the draft and re-present. Iterate until the user approves.

### Step 4: Scope Guard

Before final submission, analyze the collected content for scope creep:
- Does the bug report describe multiple independent defects?
- Does the feature request bundle unrelated changes?

If multi-concept issues are detected:
1. Inform the user: "This issue appears to cover multiple distinct topics. Focused, single-concept issues are preferred."
2. Break down the distinct groups found.
3. Offer to file separate issues for each group, reusing shared context (environment, etc.).
4. Let the user decide: proceed as-is or split.

### Step 5: Construct Issue Body

Build the issue body as markdown sections matching the template structure. For each `## ` section from the template, in order:

```markdown
## <Section Header>

<value>
```

For optional fields with no content, use the original placeholder text from the template.

### Step 6: Final Preview and Submit

Show the final constructed issue (title + labels + full body) for one last confirmation.

Then submit using a HEREDOC for the body to preserve formatting:

```bash
gh issue create --title "<title>" --label "<labels from frontmatter>" --body "$(cat <<'ISSUE_EOF'
<body content>
ISSUE_EOF
)"
```

Return the resulting issue URL to the user.

### Important Rules

- **Always read the template file** — never assume field names or structure. The templates are the source of truth and may change over time.
- **Never include personal/sensitive data** in the issue. Redact secrets, tokens, API keys.
- **One concept per issue** — enforce the scope guard.
- **Auto-detect, don't guess** — use real command output for environment fields.
- **Match the template structure** so issues look consistent whether filed via web UI or this skill.
