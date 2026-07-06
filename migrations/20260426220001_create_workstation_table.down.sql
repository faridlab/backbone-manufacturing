-- Down: drop manufacturing.workstations table
DROP TABLE IF EXISTS manufacturing.workstations CASCADE;
DROP FUNCTION IF EXISTS manufacturing.workstations_audit_timestamp() CASCADE;
