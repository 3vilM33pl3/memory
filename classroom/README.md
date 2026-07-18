# Memory Layer classroom pack

Everything a teacher needs to run Memory Layer with a class on **one
machine, with no cloud accounts, no API keys, and no per-student setup**.
The only prerequisite is Docker (with the compose plugin).

The pack pairs with the three-lesson cognitive-science curriculum on the
docs site (`/docs/classroom`): activation & decay, spreading activation,
and consolidation — taught with the live memory graph as the instrument.

## What's in the box

| File | Purpose |
| --- | --- |
| `compose.classroom.yaml` | Overlay on the root `compose.yaml` adding the student-mode toggle. |
| `seed.sh` | Seeds the shared `classroom` exercise project (keyless). |
| `worksheet.md` | Printable student worksheet for the three lessons. |

## Setup (teacher, ~10 minutes, once)

From the repository root:

```bash
# 1. Start the stack (PostgreSQL + service + web UI on port 4040).
docker compose -f compose.yaml -f classroom/compose.classroom.yaml up -d

# 2. Seed the shared exercise project.
./classroom/seed.sh

# 3. Check: open http://localhost:4040, project "classroom".
```

Everything runs keyless: retrieval is lexical and answers are extractive
with citations. No data leaves the machine.

## Two class modes

**Mode A — shared read-only set (recommended for lessons 1–2).**
Everyone explores the same curated project; nobody can change it:

```bash
CLASSROOM_READ_ONLY=true docker compose -f compose.yaml -f classroom/compose.classroom.yaml up -d
```

The web UI shows a "Student mode" banner and every write returns a clear
403. Queries still reinforce activation — students *will* see the graph
heat up as the class asks questions, which is exactly what lesson 1
teaches. Turn write access back on by re-running the same command with
`CLASSROOM_READ_ONLY=false` (or unset).

**Mode B — per-student sandboxes (for lesson 3 and free exploration).**
Leave writes enabled and give each student their own project name in the
web UI (e.g. `alice`, `bora`). Seeding a personal copy of the corpus:

```bash
./classroom/seed.sh alice
```

Projects are isolated namespaces in the same database; students cannot
disturb each other's memory sets. Note: with writes enabled, any student
can technically write to any project — for adversarial classes stay in
Mode A, and see the multi-user decision doc
(`docs/developer/adr/0006-shared-classroom-multi-user.md`).

## During class

Print `worksheet.md` (it renders fine with any Markdown-to-PDF tool, e.g.
your editor's print preview) — one copy per student or pair. Each lesson
on the docs site has a matching teacher script with the exact clicks and
expected observations.

## Reset

- Reset just the exercises: `./classroom/seed.sh` again (refreshes the
  `classroom` project), or seed fresh project names.
- Full reset (wipes ALL Memory Layer data in the stack's database):

```bash
docker compose -f compose.yaml -f classroom/compose.classroom.yaml down -v
```

Then repeat setup. The `-v` flag deletes the `pgdata` volume — don't use
it if the machine's stack is also used for real projects.

## Troubleshooting

- **Port 4040 already in use** — another Memory Layer install is running;
  stop it or change the published port in `compose.yaml`.
- **Writes fail with 403** — the stack is in student mode; restart with
  `CLASSROOM_READ_ONLY=false`.
- **`seed.sh` fails with 403** — same cause: seed before enabling
  read-only mode.
- **Answers feel terse** — that's keyless mode: extractive, cited answers
  with honest refusals. It is the mode the lessons are written for; LLM
  synthesis is optional and needs an API key (see `compose.yaml` header).
