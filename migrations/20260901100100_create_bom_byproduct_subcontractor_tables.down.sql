-- Reverse the byproduct + subcontractor creates.
DROP TRIGGER IF EXISTS bom_subcontractors_update_audit ON manufacturing.bom_subcontractors;
DROP TRIGGER IF EXISTS bom_subcontractors_insert_audit ON manufacturing.bom_subcontractors;
DROP FUNCTION IF EXISTS manufacturing.bom_subcontractors_audit_timestamp();
DROP TABLE IF EXISTS manufacturing.bom_subcontractors;

DROP TRIGGER IF EXISTS bom_byproducts_update_audit ON manufacturing.bom_byproducts;
DROP TRIGGER IF EXISTS bom_byproducts_insert_audit ON manufacturing.bom_byproducts;
DROP FUNCTION IF EXISTS manufacturing.bom_byproducts_audit_timestamp();
DROP TABLE IF EXISTS manufacturing.bom_byproducts;
