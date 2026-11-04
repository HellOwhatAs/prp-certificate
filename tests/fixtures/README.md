# Public regression fixtures

`public/q1-routes.csv` and `public/q1-assignments.csv` are an intentionally ordinary
feasible solution built entirely from `public/data/instance.json`. The generator
uses T3 aircraft, one destination per flight, A01 for passengers with no fixed
airport, and refuelling stops only where needed. It transports all 1,600 people in
136 flights, taking 23,638 aircraft minutes. It is not an optimized reference or
a claimed competition result.

Reproduce the CSV files and their pinned nine scores without changing them:

```sh
node scripts/generate-public-fixture.mjs --check
```

`--write` intentionally regenerates the fixture and its expected scores. Review
such changes together; do not use it to mask an unexpected checker regression.
`--job .cache/public-regression/job-q1.json` also creates a local input for the
native/browser generation tests.

`certificates/public-q1.json` is a **real Groth16 certificate**, produced from this
public solution using the existing Q1 proving parameters. It verifies against the
application's pinned verification key. No setup has been rerun. The proof and
nonce are random; regenerating the certificate must preserve its verified scores,
not its bytes. It contains only public claims, as all application certificates do.

Normal Rust tests and the browser WASM/QR regression verify this certificate and
reject tampering. Release CI also generates fresh native, parallel-browser and
single-thread-browser Q1 proofs. All three setup matrix digests are checked in
both PR and Release CI.

Q2/Q3 full-generation fixtures are **not present**. Their matrix tests do not
replace successful proof generation. The manual `proof-regression.yml` workflow
requires all three private jobs and fails if any are missing. Private organizer
solutions, jobs, scores and reports belong outside this directory and must not be
added to the source archive. See README.md for the manual regression procedure.
