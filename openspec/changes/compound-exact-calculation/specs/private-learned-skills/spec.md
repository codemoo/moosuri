## ADDED Requirements

### Requirement: Explicit bounded exact calculation graph
The tool SHALL evaluate2–4 exact sum/difference/product operations using fresh decimal inputs and explicit prior-result references. Every input and operation SHALL contribute to the final result. Graphs SHALL contain no literal operands or executable code, and at most32 operand references. Existing input, overflow and run-budget bounds SHALL apply while preserving generated intermediate precision.

#### Scenario: Line items followed by an adjustment
- **WHEN** fresh quantities/prices and an explicit fresh adjustment factor are supplied to a valid graph
- **THEN** the tool SHALL evaluate each operation exactly, retain per-operation private child receipts and return actual intermediate/final results
- **AND** charge one existing tool unit per actual operation while replaying a saved parent slot without new execution/charge.

#### Scenario: Invalid reference or exhausted budget
- **WHEN** a graph uses a self/forward reference, unused input/operation, excessive bounds or insufficient run budget
- **THEN** execution SHALL reject without manufacturing a successful result or learned proof.

### Requirement: Compound learning preserves structure only
A private compound route SHALL require two independent finalized runs whose actual graph receipts recompute correctly and whose final exact result is adopted in stored output. It SHALL retain only canonical graph structure and existing opaque proof digests/run links. Numerical equality between primitive results and later literal inputs SHALL never imply an edge.

#### Scenario: Independent A/B followed by fresh C
- **WHEN** two independent adopted inputs establish one graph and a later authenticated foreground run supplies unseen inputs
- **THEN** the existing learned skill pin SHALL execute that structure only with fresh C inputs
- **AND** neither A/B operands nor arbitrary hidden constants SHALL be present in its route or C discovery metadata.

#### Scenario: Proof withdrawal and notation replay
- **WHEN** equivalent numeric notation or commutative replay repeats one canonical input
- **THEN** it SHALL not establish a second independent proof.
- **WHEN** a proof thread is withdrawn
- **THEN** existing proof/dependency invalidation SHALL demote or invalidate availability and revoke derived output that actually used the route.

### Requirement: Structural private sharing
Existing explicit sharing SHALL reconstruct only the supported typed graph and repository-owned synthetic examples. No private numbers, prompts, identities, credentials, run links or success proofs SHALL be exported. Imported copies SHALL remain ordinary instruction packages with no native learned proof or additional authority.

#### Scenario: Share a compound route
- **WHEN** the owner previews/downloads the existing share package
- **THEN** it SHALL contain the graph, fresh-input instructions, synthetic example, manifest and license accepted by existing package validation
- **AND** private training content/operands and authority SHALL remain absent.
