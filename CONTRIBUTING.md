# Contributing

Conventions for changes to this repository, for humans and coding agents alike.
The disclosure policy in [PUBLIC_DEVELOPMENT.md](PUBLIC_DEVELOPMENT.md) outranks
this file.

This file states what the repository decides, and only that. What installed
tooling decides (a plugin's agents) travels with the tool and is not restated
here. What one setup decides (where a second checkout lives) binds nobody; where
a recipe here fixes such a choice, it is said as a fact about the recipe. Your
own setup goes where the tree does not track it — for Claude Code, a
`CLAUDE*.md` of your own, which `.gitignore` keeps out.

## The loop

```text
issue -> just worktree-new -> implement -> just check
      -> review locally -> commit
      -> git fetch origin -> just pre-push
      -> write the PR body to a file -> hand over push / PR
```

Each step is a section below.

## Issues

Assign at least one label when you open an issue.

| Label         | The change                                           |
| ------------- | ---------------------------------------------------- |
| `bug`         | behaviour contradicts what it promises               |
| `enhancement` | behaviour that does not exist yet                    |
| `refactor`    | behaviour unchanged                                  |
| `chore`       | production code untouched — CI, tests, tooling, docs |

## Branches

Never work on `main`. One branch per issue, cut from `origin/main` after a fetch
and named `<type>/<slug>`, where `<type>` is `ci`, `fix`, `feat` or `docs`.

```bash
just worktree-new <type> <slug>
```

The recipe fetches, adds a worktree for a new `<type>/<slug>` branch cut from
`origin/main` in a directory `.gitignore` covers (the `Justfile` names it), and
runs `just branch-check` to verify the base. Run it from the main checkout, and
remove the worktree once the branch is merged. A branch in the checkout you
already have is equally fine; that choice is your setup's.

## Verification

`just check` is the definition of green, and CI runs it and nothing else. It
formats, lints with warnings denied, tests (the architecture rules in
`arch-lint.toml` and the doctests included), builds the documentation with
warnings denied (`just rust-doc`), and runs `cargo deny`.

Report what was actually run. "I did not verify X" is a usable report; a green
claim resting on a recipe nobody ran is not.

## Commits

```text
<subject: what changed, one line>

<prose: the problem, why this fix and not the alternative, what it
cost. This is where the reasoning lives.>

Verified: <the recipes actually run, and their outcome>

Refs #<issue>
```

- No AI attribution of any kind — no `Co-Authored-By`, no "Generated with", no
  `Signed-off-by`. `just commit-msg-check` looks for them.
- `cargo fmt` output and clippy fixes go in their own commits, separate from
  behaviour changes.
- Update `CHANGELOG.md` under `## [Unreleased]` as its own commit.
- Never commit working notes, another checkout, or local agent state;
  `.gitignore` names them. If a commit needs `git add -f`, stop: something is
  filed wrong.

## Review

Review the change locally before it goes up, by a person or by an AI agent —
whichever you have. Ask three questions of every diff:

1. Does it do what its issue asked?
2. May everything in it be published?
   ([PUBLIC_DEVELOPMENT.md](PUBLIC_DEVELOPMENT.md#agent-publication-check))
3. Are the comments beside the code still true?

Say in the PR body who or what reviewed it, or that nobody did.

## Pull requests

An agent's part ends with everything that does not write to anything remote,
the gate included:

1. `git fetch origin`, then `just pre-push` (`branch-check`, `commit-msg-check`,
   `check`). Neither writes to anything remote, so being denied `git push` is no
   reason to skip them. Report the result.
2. Write the PR body to a file outside the tree and say where it is, so the
   command handed over is `--body-file <path>` and the human reads the same
   bytes that will be posted.
3. Hand over the literal commands. These two are the only ones that write to
   anything remote:

   ```bash
   git push -u origin <branch>
   gh pr create --base main --head <branch> \
     --title "<subject>" --body-file <path>
   ```

The PR body records what changed, what was verified (or that it was not), and
what it deliberately does not cover, under the same disclosure policy as
everything else.

A PR title reaches `main`'s merge commit, and GitHub reads a CI skip keyword
there as it reads one in a message: the run is skipped, its checks stay
_pending_ rather than failing, and nothing looks wrong. Write "the skip keyword"
instead, or name the mechanism.

Releases and publishes (crates, the repository's own settings) are performed by
a human hand.

## Documentation

Requirements, decisions and verifications are whyshelf books: the documents
under `books/`, declared in `.whyshelf/books.toml`, with one decision per file in
`.whyshelf/ledger/`. Write them in public-safe English (see
PUBLIC_DEVELOPMENT.md) and check them with `whyshelf books check`.

Everything else is RustDoc: crate and module docs (`//!`) and item docs (`///`),
with diagrams as ASCII in code blocks tagged `text`; concepts, file formats,
settings, the command line, compatibility and known limits sit beside the code
they describe. `cargo doc` must be warning-free (`just rust-doc`, part of
`just check`). When two texts disagree the code wins, then the doc comment; a
comment contradicting an issue is a finding about the issue.

Four rules for writing that prose:

- **State a rule once, where the thing it constrains is defined.** Elsewhere,
  link to it and write only what that site adds. A second full statement is the
  copy nobody edits the day the rule moves.
- **Do not write the state of the tree where nothing maintains it.** "Not yet",
  "the only caller" are true when written and go false in silence. Keep the rule
  and cut the clause about today.
- **A number describing a list belongs in the file holding the list, or
  nowhere.** Point at the list instead.
- **A sentence recording that a rule changed stays.** It is a constraint in the
  past tense, and it stops the next reader undoing the rule. If deleting it
  costs nothing, it is a report on how a change went, and belongs in the commit
  message.
