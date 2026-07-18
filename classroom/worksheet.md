# Memory Layer — student worksheet

Name: ______________________  Date: ______________

You are exploring a running memory system that borrows its design from
cognitive science. Everything you observe on screen — glowing nodes,
moving pulses, growing trees — is a real mechanism, not a metaphor.

Open **http://localhost:4040** and select the project your teacher gives
you (usually `classroom`).

---

## Lesson 1 — Activation and decay

Memories that get used become easier to recall; memories that don't fade.
The system scores every memory with an *activation* value that rises on
use and decays exponentially with time — the same shape as human
forgetting curves.

1. Open the **Graph** tab and turn off the Code and Provenance layers so
   only Memory nodes show. Look at the node sizes and colors.
   Which memory looks "hottest" right now? Write its title:

   ________________________________________________________________

2. Go to the **Query** tab and ask: *"How does reinforcement work?"*
   Read the answer and note which memories it cites (the [1] [2] markers):

   ________________________________________________________________

3. Ask the same question two more times, then return to the Graph tab
   and refresh. What changed about the cited memories' nodes?

   ________________________________________________________________

4. **Think:** if nobody queried this project for a month, what would the
   graph look like? Why might forgetting be *useful* for a memory system?

   ________________________________________________________________

## Lesson 2 — Spreading activation

Recalling one memory makes related memories easier to recall too:
activation *spreads* along links, weaker with each hop.

5. In the Graph tab, click the memory about **consolidation**. Watch the
   pulse travel along its links. List two memories it is linked to:

   ________________________________________________________________

6. Query: *"What does the value gate decide?"* — then check the graph.
   Did any memory warm up that the answer did **not** cite directly?
   Which one, and what connects it?

   ________________________________________________________________

7. **Think:** when you smell a place you knew as a child and suddenly
   remember people and sounds from it, what just happened in your head,
   in this lesson's terms?

   ________________________________________________________________

## Lesson 3 — Consolidation (making sense of many memories)

Brains don't keep every episode forever — sleep consolidates related
episodes into general knowledge. This system does the same: it finds
clusters of related memories and proposes one higher-level **insight**
memory that summarizes them — but a human must approve it.

8. Ask your teacher to run `memory structure --project classroom` on the
   projector (or run it yourself in Mode B). How many groups did the
   scan discover, and what do the members of one group have in common?

   ________________________________________________________________

9. The project already contains one memory of type `insight`. Find it
   (Memories tab, filter by type). What claim does it make, and why is
   it different in kind from the other memories?

   ________________________________________________________________

10. **Think:** why does the system require a human to approve each
    insight instead of writing it automatically? What could go wrong
    without that gate?

    ________________________________________________________________

---

**Exit ticket:** in one sentence each, define *activation*, *spreading
activation*, and *consolidation* — using something from your own life,
not the software, as the example.
