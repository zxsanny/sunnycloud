# Token type prefixes for every token kind

**Task**: AZ-4268_token_type_prefixes
**Name**: Token type prefixes
**Description**: Every token kind carries its own short type prefix, generated and checked in one place (`sc_ids`).
**Complexity**: 2 points
**Dependencies**: AZ-3692
**Component**: helper `sc_ids`, 01 identity, 06 sharing
**Tracker**: AZ-4268
**Epic**: AZ-3623 (E-01)

## Problem

Found by AZ-3692 (ST-01-5) on 2026-11-13: `data_model.md` says tokens carry a type prefix, but `sc_ids::new_token` returns bare base64 and only share links add `sc_shr_` themselves. Owner decision 2026-11-13: add a prefix per kind (nothing is deployed, so no stored token breaks).

## Acceptance Criteria

**AC-1: One prefix per kind**
Given `sc_ids::new_token`
When it makes a token of each kind
Then the token starts with its kind's prefix: session `sc_ses_`, refresh `sc_ref_`, device `sc_dev_`, invite `sc_inv_`, setup `sc_set_`, login code `sc_lgc_`, device code `sc_dcd_`, share `sc_shr_`; the share service no longer adds its own

**AC-2: Another kind's token is refused**
Given a token of one kind
When it is presented where another kind is expected
Then it is refused like an unknown token

**AC-3: The suite's test passes**
Given `token_storage::every_token_family_carries_its_own_type_prefix`
When it runs without `#[ignore]`
Then it passes

**AC-4: Hashes**
Given any token
When it is stored
Then only the hash of the full presented string is stored
