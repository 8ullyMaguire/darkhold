-- M16-11: the paid marketplace, separate from credits.
--
-- Credits are the closed-loop economy (M45-18) and this is a different ledger on purpose: a credit balance is not money and must never be
-- readable as a price. Nothing here references a credit balance, and nothing in the credits tables references this one.
--
-- A payment is recorded **pending** and captured later, because there is no payment provider. `processor` says which port took it ("manual"
-- is a real answer, not a placeholder) and `processor_ref` is what reconciles against a bank statement.
--
-- Money is BIGINT, not INTEGER: minor units overflow INT4 at ~21 million euros, and a ledger that cannot represent a sale is not a ledger.
--
-- `refunded_minor` is cumulative, not per-refund, so the one check that matters (`refunded <= captured`) cannot drift; the `refunds` rows
-- keep the individual events and their reasons.
CREATE TABLE payments (
    id             TEXT PRIMARY KEY,
    account        TEXT NOT NULL,
    subject_kind   TEXT NOT NULL,          -- 'extension' today; a work purchase has its own path
    subject_id     TEXT NOT NULL,
    subject_version TEXT,
    amount_minor   BIGINT NOT NULL CHECK (amount_minor > 0),
    currency       TEXT NOT NULL,
    state          TEXT NOT NULL DEFAULT 'pending',   -- pending|captured|refunded|cancelled
    processor      TEXT NOT NULL DEFAULT 'manual',
    processor_ref  TEXT,
    idempotency    TEXT NOT NULL,
    tax_minor      BIGINT NOT NULL DEFAULT 0,
    created_at     TEXT NOT NULL,
    captured_at    TEXT,
    refunded_minor BIGINT NOT NULL DEFAULT 0
);

-- One payment per (account, idempotency key): a retried purchase must not charge twice, and the retry is what a client does on a timeout.
CREATE UNIQUE INDEX idx_payments_idempotency ON payments(account, idempotency);
CREATE INDEX idx_payments_account ON payments(account, created_at);
CREATE INDEX idx_payments_state ON payments(state);

CREATE TABLE refunds (
    id           TEXT PRIMARY KEY,
    payment_id   TEXT NOT NULL REFERENCES payments (id),
    amount_minor BIGINT NOT NULL CHECK (amount_minor > 0),
    reason       TEXT NOT NULL,
    actor        TEXT NOT NULL,
    created_at   TEXT NOT NULL
);

CREATE INDEX idx_refunds_payment ON refunds(payment_id);

-- What a package costs. Not a manifest field: the manifest is what the author signed and reviewed, while a price is the seller's offer and
-- changes without re-review. `enabled` is what makes a package free again without deleting its history.
CREATE TABLE extension_prices (
    manifest_id  TEXT PRIMARY KEY,
    price_minor  BIGINT NOT NULL CHECK (price_minor >= 0),
    currency     TEXT NOT NULL,
    enabled      INTEGER NOT NULL DEFAULT 0,
    set_by       TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);

-- The operator's tax identity, as an invoice must print it. One row: a host has one registration, and a second row would mean two answers to
-- "what rate do we charge".
CREATE TABLE operator_tax_profile (
    id             TEXT PRIMARY KEY DEFAULT 'default',
    legal_name     TEXT NOT NULL,
    country        TEXT NOT NULL,
    tax_id         TEXT,
    rate_bp        BIGINT NOT NULL CHECK (rate_bp BETWEEN 0 AND 10000),
    invoice_prefix TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);
