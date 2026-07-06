-- Down: drop manufacturing.boms table
DROP TABLE IF EXISTS manufacturing.boms CASCADE;
DROP FUNCTION IF EXISTS manufacturing.boms_audit_timestamp() CASCADE;
