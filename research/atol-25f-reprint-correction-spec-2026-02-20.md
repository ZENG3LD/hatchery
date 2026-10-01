# ATOL 25F: Spec for Automated Receipt Re-Issue / Correction

Date: 2026-02-20
Status: Draft for implementation

## 1. Goal

Build a one-click pipeline that:
1. Takes exported OFD receipts in JSON (historic documents).
2. Transforms them into valid correction receipt payloads.
3. Sends payloads to ATOL cash register flow (primary target: ATOL 25F environment) and prints fiscal documents.

Primary scenario is **fiscal correction**, not non-fiscal "copy print".

## 2. Regulatory and Product Context

- ATOL positions KKT work under 54-FZ and fiscal format requirements (FFD).  
  Source: ATOL legal context page with 54-FZ references: `https://www.atol.ru/company/service-support/dkkt10-platforma5/`
- ATOL Online documentation describes correction logic and required reason fields for correction receipts.  
  Source: `https://help.atol.online/article/11243`
- For FFD 1.2, correction often requires a two-step sequence (reverse old values, then apply correct values).  
  Source: `https://help.atol.online/article/11243`

## 3. Integration Capabilities to Use

- ATOL Driver stack (DKKT 10 / Platform 5) explicitly lists integration modes including `JSON + WEB` and Web server mode.  
  Source: `https://www.atol.ru/company/service-support/dkkt10-platforma5/`
- ATOL Online API supports JSON-based receipt registration and provides schema endpoints.  
  Sources:
  - `https://f1.atol.ru/api/`
  - `https://online.atol.ru/possystem/v5/schema`
- ATOL integration docs hub: `https://integration.atol.ru/`

## 4. Input and Output Contracts

### 4.1 Input (from OFD export)

Input resembles tag-based fiscal document JSON, for example:
- root `document`
- items in tag `1059`
- values like `1030` (name), `1079` (price), `1043` (amount), `1212`/`1214` (payment attributes), `2108` (measure)

Example source file used in analysis:
- `C:\Users\VA PC\Downloads\Telegram Desktop\7380440801603539_6662.json`

### 4.2 Output (correction payload)

Target payload shape (example already available):
- `operation: "sell_correction"`
- `external_id`
- `correction` object with reason/date/doc number/type
- `receipt` with `company`, `items`, `payments`, `total`

Example source file used in analysis:
- `C:\Users\VA PC\Downloads\Telegram Desktop\7380440801603539_6662_correction.json`

## 5. Mapping Rules (OFD -> Correction Payload)

Minimal deterministic mapping for current sample format:

| OFD tag/path | Meaning | Correction JSON field |
|---|---|---|
| `document.1018` | seller INN | `receipt.company.inn` |
| `document.1009` | settlement address | `receipt.company.payment_address` |
| `document.1054` | tax system | `receipt.company.sno` |
| `document.1020` or `document.1081` | total amount | `receipt.total` |
| `document.1059[*].1030` | item name | `receipt.items[*].name` |
| `document.1059[*].1023` | qty text | `receipt.items[*].quantity` (normalized number) |
| `document.1059[*].1079` | item price | `receipt.items[*].price` |
| `document.1059[*].1043` | item amount | `receipt.items[*].amount` |
| `document.1059[*].1212` | payment object | `receipt.items[*].payment_object` |
| `document.1059[*].1214` | payment method | `receipt.items[*].payment_method` |
| `document.1059[*].2108` | measure code | `receipt.items[*].measure_code` |
| `document.1105`/payment tags | paid total | `receipt.payments[0].sum` |

Notes:
- Keep money in integer minor units exactly as source uses (no float).
- Trim accidental spaces in text fields (`inn`, names, fiscal numbers).
- Preserve exact fiscal attributes unless explicit correction requires change.

## 6. Correction Strategy

### 6.1 If task is legal correction (recommended)

Implement correction receipt flow with required reason metadata:
- base document date/number
- correction type (self/prescription)
- human-readable reason

For FFD 1.2: support two-step correction mode where required:
1. Reverse wrong amount/details.
2. Register corrected values.

Reference: `https://help.atol.online/article/11243`

### 6.2 If task is "duplicate physical print only"

Treat as separate mode and do not mix with fiscal correction:
- duplicate print can be operationally different from issuing correction docs;
- legal effect is different.

This mode must be explicitly approved by responsible accountant/legal owner before production.

## 7. Non-Functional Requirements

- Idempotency: each generated correction must have stable unique `external_id`.
- Batch safety: continue on per-receipt errors; write fail list; no global abort.
- Auditability: store input JSON hash, output payload, device response, timestamp.
- Retry policy: network/driver retries with bounded attempts and backoff.
- Dry-run mode: validate and generate output without sending to KKT.

## 8. Delivery Plan

1. Implement `transform_ofd_to_correction()` converter.
2. Implement validator against required fields and enum domains.
3. Add sender adapter (`ATOL transport` abstraction) with:
   - `dry_run`
   - `send_one`
   - `send_batch`
4. Add CLI one-click command:
   - input folder
   - output folder
   - dry-run / send
5. Add report generator:
   - success count
   - failed count + reasons
   - list of generated external IDs

## 9. Complexity Estimate

- MVP for one known OFD format and one correction scenario: 2-5 working days.
- Hardened batch production tool (validation, retries, logs, edge cases): 5-10+ working days.

Main risk factors:
- exact driver/API profile in real customer setup;
- FFD version constraints in production environment;
- correctness of correction grounds and fiscal attributes.

## 10. Acceptance Criteria

- Given N source OFD JSON files, pipeline produces N valid correction payloads.
- In dry-run, 100% payloads are validated with explicit pass/fail report.
- In send mode, each payload has stored device/API response and traceable `external_id`.
- Re-run with same input does not duplicate already successful operations.

## 11. Sources (official links)

1. ATOL DKKT 10 / Platform 5: `https://www.atol.ru/company/service-support/dkkt10-platforma5/`
2. ATOL integration portal: `https://integration.atol.ru/`
3. ATOL Online API docs hub: `https://f1.atol.ru/api/`
4. ATOL Online API schema root: `https://online.atol.ru/possystem/v5/schema`
5. ATOL help: how to create checks: `https://help.atol.online/article/11242`
6. ATOL help: correction check logic (including FFD 1.2 notes): `https://help.atol.online/article/11243`
7. Driver developer rules PDF: `https://f1.atoldriver.ru/doc/%D0%9F%D0%A0%D0%90%D0%92%D0%98%D0%9B%D0%90%20%D0%98%D0%A1%D0%9F%D0%9E%D0%9B%D0%AC%D0%97%D0%9E%D0%92%D0%90%D0%9D%D0%98%D0%AF%20%D0%B4%D1%80%D0%B0%D0%B9%D0%B2%D0%B5%D1%80%D0%B0%20%D0%B4%D0%BB%D1%8F%20%D1%80%D0%B0%D0%B7%D1%80%D0%B0%D0%B1%D0%BE%D1%82%D1%87%D0%B8%D0%BA%D0%BE%D0%B2.pdf`

