-- Down: drop manufacturing.repair_parts table
DROP TABLE IF EXISTS manufacturing.repair_parts CASCADE;
DROP FUNCTION IF EXISTS manufacturing.repair_parts_audit_timestamp() CASCADE;
