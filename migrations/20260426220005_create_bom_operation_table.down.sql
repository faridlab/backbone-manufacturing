-- Down: drop manufacturing.bom_operations table
DROP TABLE IF EXISTS manufacturing.bom_operations CASCADE;
DROP FUNCTION IF EXISTS manufacturing.bom_operations_audit_timestamp() CASCADE;
