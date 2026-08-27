-- Down: drop manufacturing.bom_subcontractors table
DROP TABLE IF EXISTS manufacturing.bom_subcontractors CASCADE;
DROP FUNCTION IF EXISTS manufacturing.bom_subcontractors_audit_timestamp() CASCADE;
