-- Down: drop manufacturing.workstation_losses table
DROP TABLE IF EXISTS manufacturing.workstation_losses CASCADE;
DROP FUNCTION IF EXISTS manufacturing.workstation_losses_audit_timestamp() CASCADE;
