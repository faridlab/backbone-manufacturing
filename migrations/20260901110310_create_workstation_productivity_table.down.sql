-- Down: drop manufacturing.workstation_productivity table
DROP TABLE IF EXISTS manufacturing.workstation_productivity CASCADE;
DROP FUNCTION IF EXISTS manufacturing.workstation_productivity_audit_timestamp() CASCADE;
