# Object explorer delivery

User scope: implement all roadmap points except AI; skip manual testing for now.
Automated tests/build/review remain required. Preserve unrelated in-progress
architecture and minisign changes in the working tree.

First deliver typed relation folders from existing provider metadata: Tables,
Views, Materialized Views and Other Relations. Reuse existing query/metadata
navigation and connection identity. Keep unknown relation kinds visible rather
than pretending they are tables. Use independent expansion keys for folders.
No driver public API or persisted format changes in this step.

Next extend provider object discovery/source navigation to sequences, routines,
triggers and types, then object search/favorites and schema editing. Provider
capabilities and real catalog data must determine supported object kinds. Any
new public catalog contract requires a concrete design review before changes.
P1-01 remains open until all its object types and actions are wired.

## Implementation and review

Four relation folders now classify native type labels, keeping unknown kinds in
Other Relations. Expansion keys encode schema length/name and category, preventing
collisions for quoted schema names containing separators. Existing connection,
schema and object navigation paths are reused. No new database query is added;
each category filters already-loaded metadata. Folders do not imply provider
discovery support, and empty states explicitly refer to loaded objects.

Manual testing is skipped as explicitly requested by the user; no visual or live
provider acceptance is claimed. Existing in-memory table-folder expansion keys
change to category-qualified keys; no persisted data or config changes.

Self-review: correctness has native-kind/unknown/key-collision unit assertions;
performance remains linear in loaded relation count with four fixed categories;
security adds no SQL, logging, auth or external calls; robustness uses existing
owned metadata with no resources/dependencies; syntax follows existing GPUI
patterns; APIs are existing supported methods; category semantics are explicit;
tests assert classifications and independent keys, manual testing skipped;
design confines category logic to schema rendering.

Validation: formatting, Clippy workspace/all-targets, app build and full workspace
tests passed with `--locked` (336 harness passes; unconfigured provider/keychain
tests remain early returns). Independent review found default expansion producers
still using legacy raw schema names. Both NavigatorState initialization and
connection initialization now use a shared table-folder key; regression assertions
confirm initial public Tables expansion without expanding Views. Re-review found
no remaining actionable issue. `git diff --check` passed. Manual app/GUI testing
was skipped per explicit user instruction; native display behavior is unverified.
No commits or pushes were made for this delivery.
