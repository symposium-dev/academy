# Maintaining This Book

This page is the **update contract** for the AI agent (and human) that edits this book.
Much of Jamsession is developed conversationally with an agent, drawing on the source code,
CI, and design documents. This book is a **source of truth** for Jamsession's design and
status, so it only stays useful if it moves in lockstep with the system. When the system
changes, update the docs in the *same* session — treat it as part of the change, not a
follow-up.

The rest of this page is written in the imperative, addressed to that agent.

## How the pieces relate — RFDs vs. the Architecture section

Jamsession keeps two kinds of design document, and they are **different tenses of the same
information**, not competitors:

| | RFD | Architecture & Design |
|---|---|---|
| Describes | a *change* (a delta) | the *end-state* (the whole) |
| Tense | "we **will** change X" | "X **is** built this way" |
| Lifespan | historical once completed | living, always current |
| Answers | *what are we doing, and why?* | *how is it built, right now?* |

An **RFD** proposes and reviews a change. Once its implementation lands and it moves to
*Completed*, it becomes a historical record — a snapshot of one decision at one time. The
**Architecture & Design** section is where the accumulated result of all those changes lives
as a single, present-tense, coherent picture. Design flows **from** the architecture (an RFD
is a proposed delta against the current design) and back **into** it (a completed RFD's
durable design is reflected in the architecture pages, so the knowledge survives the RFD
receding into history).

Practical consequence: **do not leave the current design of a subsystem reconstructable only
by replaying RFDs.** When an RFD completes, make sure the durable "how it works now" lives in
an `architecture/` page in present tense.

## When to update — trigger → page

When one of these lands, update the matching page(s) before you consider the work done:

| A change in the system | Update these pages |
|---|---|
| A larger change is being planned | Open an **RFD** (`md/rfds/<name>/`) per the [RFD process](../rfds/README.md); track steps in its `implementation.md` |
| An RFD's implementation step lands | Tick the step in that RFD's `implementation.md` |
| An RFD completes | Move it to *Completed* in [`SUMMARY.md`](../SUMMARY.md); ensure its durable design is reflected present-tense in the relevant [architecture](../design/README.md) page |
| Observable behavior changes / something ships | The relevant [architecture](../design/README.md) page (state it as it now works) |
| A new subsystem, flow, or mechanism is built | Add/'update the matching [architecture](../design/README.md) page or flow diagram |
| A new module, or a change in how modules relate | The **Module map** in [architecture](../design/README.md) |
| A new term worth defining | [`terminology.md`](../terminology.md) |
| Any new page | Register it in [`SUMMARY.md`](../SUMMARY.md) — a page not listed there does not render |

When a change touches more than one row, update all of them in the same change.

## Conventions

- **Architecture pages are present-tense and factual.** They describe how the system works
  *today*, grounded in the real modules. Aspirational or "we will" content belongs in an RFD.
- **Ground every claim in the code.** Tie statements to actual modules/files and, where the
  page uses them, keep the code-anchor references accurate.
- **Every page is in the nav spine.** Add its [`SUMMARY.md`](../SUMMARY.md) entry in the same
  edit that creates the page.
- **Diagrams use Mermaid** in fenced ` ```mermaid ` blocks.
- **Style** (inherited from the RFD process): no promotional or dramatic language; be factual
  and brief; lead with concrete concepts, then generalize; include examples.

## Verify before you commit

Build the book and confirm your pages render and the nav is intact:

```bash
mdbook serve
```

Confirm the new/edited page appears in the sidebar and that content and intra-book links
resolve. Then commit.
