# P1-09 connection usability and security

P1-08 is checkpointed separately. Its import runner, source transfer, full-source
export and workspace UI remain open. Vendor ODBC validation remains deferred to
Linux.

1. Replace the SSH placeholder with authenticated, loopback-only forwarding.
   Require explicit server host-key verification. Reject failed authentication,
   bound connection setup and forwarding resources, and close the listener and
   channels when the owning connection closes. Test with a local SSH endpoint,
   including rejected host keys and credentials, forwarding and teardown.
2. Wire transport settings into both the draft Test flow and saved connections.
   Retain the database hostname for TLS verification through a tunnel. Route
   reconnects, isolated script sessions and metadata connections through the
   same transport policy. Fail explicitly for unsupported provider combinations.
3. Expose supported client certificate and authentication settings, plus typed
   advanced properties and connection timeouts. Keep provider defaults for old
   profiles. Keep secrets out of diagnostics and protect new saved credentials
   with the existing platform credential facilities where supported.
4. Add connection groups and colors. Apply read-only safeguards on every write
   entry point, using native enforcement where available. Verify SQL tabs,
   grid edits, DDL and transfer entry points before advertising the safeguard.
5. Update connection and provider documentation. Verify saved-profile
   compatibility, draft-versus-active isolation, timeout/error behavior and live
   read-back. Run workspace format, Clippy, tests and build, check application
   startup, obtain independent review and clean obsolete build artifacts. GUI
   acceptance belongs to the user.
