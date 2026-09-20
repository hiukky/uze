## MODIFIED Requirements

### Requirement: Doctor reports attachment health by plugin and harness
The system SHALL report the health of every managed package attachment by
plugin and harness, including Hook attachments. A Hook report SHALL expose
the semantic event, compatibility verdict, reason for any loss, and the
receipt-owned artifacts that UZE produced.

#### Scenario: A degraded hook is actionable
- **WHEN** a package declares a Hook whose semantics cannot be fully preserved on one harness
- **THEN** Doctor identifies that harness as degraded rather than healthy-native
- **AND** reports which semantic guarantee was weakened and which artifact was produced
