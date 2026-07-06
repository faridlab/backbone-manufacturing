-- Down: drop manufacturing.operations table
DROP TABLE IF EXISTS manufacturing.operations CASCADE;
DROP FUNCTION IF EXISTS manufacturing.operations_audit_timestamp() CASCADE;
