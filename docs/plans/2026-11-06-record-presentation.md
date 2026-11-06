# Record presentation combined with Grid and Text

## Scope

Record is an independent single-row toggle, not a third presentation. Preserve
Grid/Text while enabling, disabling or navigating Record in both table and SQL
results. Grid + Record displays Name / Row #n as a two-column table; Text + Record
uses the same aligned text formatter and monospace body as ordinary results.
Keep bounded asynchronous snapshots, staged-value handling, navigation, refresh,
field paging and stale-result rejection. Keep Record read-only and prevent grid
paste/shortcuts from modifying the hidden grid. Search returns to ordinary Grid.

## Steps

1. Reproduce the mutually exclusive state transition with an automated test.
2. Separate record selection from presentation; update every caller and reset path.
3. Share table text formatting and render a transposed record in Grid or Text.
4. Cover both tab types, toggling, row navigation, result resets and field output.
5. Update guides; run formatting, Clippy, workspace tests and launch the app.
6. Obtain an independent review and leave visual GUI acceptance to the user.

Do not commit this plan or implementation until explicitly requested; project
instructions prohibit unsolicited commits. Keep the plan in its own docs commit
when authorized.
