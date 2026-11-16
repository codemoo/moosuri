## ADDED Requirements

### Requirement: Preserve mixed-round source receipts
After native signed settlement, admitted saved-source Search and Source tool results SHALL expose the same direct receipt addresses as fresh foreground results under the existing evidence-calculation profile. The address SHALL identify the original stored step and call without adding authority.

#### Scenario: Use settled evidence without repeating reads
- **GIVEN** a native-mail-enabled run with separately granted device authority and admitted evidence calculation
- **WHEN** source search/read results share rounds with native reads and the original native waits settle
- **THEN** both already-saved local results and deferred local results include their actual receipt addresses in resumed inference
- **AND** source-bound arithmetic consumes those addresses and returns its verified exact result without repeating the saved-source reads

#### Scenario: Existing projection boundaries remain
- **WHEN** a result is not a saved-source search/read or evidence calculation is not admitted
- **THEN** serialization does not add a source receipt
- **AND** typed errors retain their existing error flag and result envelope
