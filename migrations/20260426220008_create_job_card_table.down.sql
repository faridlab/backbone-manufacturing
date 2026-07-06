-- Down: drop manufacturing.job_cards table
DROP TABLE IF EXISTS manufacturing.job_cards CASCADE;
DROP FUNCTION IF EXISTS manufacturing.job_cards_audit_timestamp() CASCADE;
