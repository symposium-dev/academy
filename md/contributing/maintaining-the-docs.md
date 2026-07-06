# Maintaining This Book

This page is the **update contract** for the AI agent (and human) that edits this book.
Much of Jamsession is developed conversationally with an agent, drawing on the source code,
CI, and design documents. This book is a **source of truth** for Jamsession's design and
status, so it only stays useful if it moves in lockstep with the work. When the design or the
system changes, update the docs in the *same* session — treat it as part of the change, not a
follow-up.

The rest of this page is written in the imperative, addressed to that agent.

## How the pieces relate — the Architecture section vs. RFDs

Jamsession keeps two kinds of design document. They are not competitors and not two tenses of
the same fact; they sit at **different stages of the same pipeline**:

```text
Architecture & Design  →  RFD  →  Implementation
(work out the intended     (propose a specific,   (build the delta,
 design of the system)      reviewable delta)       track it to done)
```

| | Architecture & Design | RFD |
|---|---|---|
| Role | the *design of record* — the intended architecture of the whole | a *change* (a delta) drawn from it |
| Scope | the system, built and planned together | one specific proposal at one time |
| Answers | *how is the system meant to be built?* | *what change are we making next, and why?* |
| Lifespan | living, leads the work | historical once completed |

The **Architecture & Design** section is the upstream design workspace: it holds the coherent,
intended design of the whole system, and it is where we think a design through *before* it
becomes a change proposal. An **RFD** then carves a specific, reviewable delta out of that
design and takes it to implementation. Design flows **from** the architecture (an RFD is a
delta against the intended design) — the architecture **leads**, RFDs follow.

Two practical consequences:

- **Forward-looking design belongs in the architecture, not only in RFDs.** It is expected
  that this section runs ahead of the code and describes parts not yet built.
- **A reader must be able to tell built from planned.** Never let a not-yet-built design read
  as if it already exists — someone (or some agent) will build *on* it. Mark planned parts
  as such and let the **Build-Out Roadmap** carry the authoritative done/in-flight/planned
  status; keep the architecture prose at the design level.

## When to update — trigger → page

When one of these happens, update the matching page(s) before you consider the work done:

| Trigger | Update these pages |
|---|---|
| We work out (or revise) how part of the system *should* be designed | The relevant [architecture](../design/README.md) page — capture the intended design, marking anything not yet built as planned |
| A worked-out design is ready to build | Open an **RFD** (`md/rfds/<name>/`) per the [RFD process](../rfds/README.md) carving out the delta; track steps in its `implementation.md` |
| An RFD's implementation step lands | Tick the step in that RFD's `implementation.md` |
| An RFD completes | Move it to *Completed* in [`SUMMARY.md`](../SUMMARY.md); update the relevant [architecture](../design/README.md) page and Build-Out Roadmap so that design now reads as built |
| Observable behavior changes / something ships | The relevant [architecture](../design/README.md) page (reconcile it with what now exists) |
| A new subsystem, flow, or mechanism is built | Add/update the matching [architecture](../design/README.md) page or flow diagram |
| A new module, or a change in how modules relate | The **Module map** in [architecture](../design/README.md) |
| A cross-cutting, load-bearing decision is made or changed | Add/update an entry in [Architecture decisions](../design/decisions.md) with a new `D<n>` code; a feature-local decision stays in its RFD and is linked from there |
| A new term worth defining | [`terminology.md`](../terminology.md) |
| Any new page | Register it in [`SUMMARY.md`](../SUMMARY.md) — a page not listed there does not render |

When a change touches more than one row, update all of them in the same change.

## Conventions

- **Architecture pages describe the intended design, and separate built from planned.** They
  may cover parts not yet implemented — that is the point of the section. But mark planned
  design clearly (a status note, or defer the detail to the Build-Out Roadmap) so nothing
  reads as built when it isn't.
- **Ground built claims in the code.** For anything described as existing, tie statements to
  actual modules/files and keep code-anchor references accurate. Planned design is grounded in
  the design discussion instead, and is labelled as planned.
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
