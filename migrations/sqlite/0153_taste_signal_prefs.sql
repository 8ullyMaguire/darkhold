-- M45-70: the reader may switch off a signal, so "explain my taste" is actionable rather than a report.
--
-- One row per (reader, signal) that they have turned OFF. Absence means on, so the table stays small and
-- the default is the one that explains a reading: a signal you never touched is a signal that counted.
-- The row has a foreign key onto `accounts`, so erasing the reader takes the preference with it.
CREATE TABLE taste_signal_prefs (
    account_id TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    signal     TEXT NOT NULL,
    enabled    INTEGER NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (account_id, signal)
);
