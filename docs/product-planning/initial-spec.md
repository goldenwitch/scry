# Initial spec

scry (lowercase intentional) is a set of tools for agents to collect, organize,
and validate a **corpus**.

## Jobs to be done

It has three primary jobs, from which the rest of the capabilities may only be
added if they accrue to these jobs.

1. Enable agents to **search** the **corpus**, receiving with every **hit** a
   **passage**: the **text** itself, and the **handle** that vouches for it.
2. Enable agents to **add** one or more **document**s to the **corpus** by
   their **origin**s, and to **delete** a document by that same **origin**.
3. Enable agents to exchange a **handle** for **provenance**: the **origin**,
   the **span** within it, and the **fetched_at** and **ttl** by which an agent
   judges the material's age.

## What the jobs settle

An **origin** is a **path** or a **url**, and it is the **document**'s identity.
Adding the same **origin** again replaces what was there. An **add** accepts a
canonical set of origins and one shared **ttl**. Each member is fetched and
upserted independently; an omitted origin is neither read nor changed, and
the operation never enumerates or deletes omitted documents.

An add report contains one result for every canonical input origin. A refused
member does not stop the operation. A processing or persistence failure stops
later members, while earlier successful replacements remain committed; an
empty set is a successful no-op.

scry holds **text** and converts nothing, so **add** refuses bytes it cannot
read as text.

Surrounding context asks for no fourth job. A **chunk** knows its **document**
and its place in it, so the **neighbours** of a **handle** are **passage**s too.

**provenance** refuses rather than answer wrongly. **Stale** says the **text**
at that **span** is no longer the text the **handle** was cut from; **Gone**
says the **document** is no longer held.
