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
issue -> just worktree-new -> books (pending) -> implement -> just check
      -> review locally (blind) -> judge approve -> commit
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
warnings denied (`just rust-doc`), runs `cargo deny`, and checks the
repository's own books (`just books`: `whyshelf check` is clean and no approval
is stale).

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

Review every change locally before it goes up, by a person or by an AI agent
other than the one that wrote it (REQ-13). Ask four questions of every diff:

1. Does it do what its issue asked?
2. May everything in it be published?
   ([PUBLIC_DEVELOPMENT.md](PUBLIC_DEVELOPMENT.md#agent-publication-check))
3. Are the comments beside the code still true?
4. Is every sentence of the books it touches true of the code (DEC-25)?

Review blind: give the reviewer the issue and the branch, not a summary of the
change, which keeps the review inside the summary. The reviewer starts from
whyshelf — `status`, `stale` and `check`, then `get` and `trace` of the documents
that name the issue (or, for a change of code alone, of the documents whose
`rustdoc` or `tests` name the changed files), and `get --approved` and `diff` of
the approved ones — follows their `rustdoc` items and `tests` into the code, and
reads the diff for what the documents did not lead to. This is the one place the
procedure is written.

Say in the PR body who or what reviewed it, that it was blind, and what it found.

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
PUBLIC_DEVELOPMENT.md) and check them with `whyshelf check`.

### Working with the books

`whyshelf map` answers the books, their fields and the links between them, as
`.whyshelf/books.toml` declares them; its `diagram` draws the books and the
links.

A change goes through the books before the code:

```text
issue -> finding -> requirement / decision / verification (pending)
      -> code and tests -> just check -> blind review -> judge approve
      -> commit (the ledger records too) -> just pre-push -> PR (Closes #<issue>)
```

- Conventions of these books that the declaration does not state: a finding
  gives the issue it came from in `source`; a requirement and a decision name
  their issues in `issues`; a decision says in `revisit` when to look at it
  again, and names in `rustdoc` the items whose docs state its rule; a
  verification's `method` `e2e` is for tests that run the binary.
- Write a document with `whyshelf new <book> --title <text> <entry>=<value>`;
  give a list item that holds a comma with `<entry>+=<item>`.
- A bug against a promise the books already make needs no new requirement or
  decision: add the finding, correct the decision it breaks, and extend the
  verification's tests.
- `serves` and `checks` cover their books, so `whyshelf check` fails while a
  requirement has no decision or a decision has no verification.
- After the review, the author approves the new documents and what the change
  made stale in one `judge approve`, upstream first, with a reason that names
  the review and says why it holds. An edit that keeps the meaning (a typo, a
  struct literal that gained a field) can be declared with `judge same`
  instead. The records in `.whyshelf/ledger/` are committed with the change.
- `whyshelf check` is clean and `whyshelf stale` is empty before a PR goes up:
  `just check`, and so CI, fails otherwise (`just books`). An approval made with
  `--until` fails it once it expires. Pending documents pass it; approve them.
- `tests` is a link to artifacts in the space `file` (`space = "file"` in
  `.whyshelf/books.toml`): its names are paths, compared as written, case
  included. Write a path as Git tracks it (`Justfile`, not `justfile`), or it
  names no file, on every system. `check` lists the names with nothing there
  (`absent`) and those that cannot be told now (`unknown`).

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
