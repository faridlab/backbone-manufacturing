-- Down: drop manufacturing.bom_byproducts table
DROP TABLE IF EXISTS manufacturing.bom_byproducts CASCADE;
DROP FUNCTION IF EXISTS manufacturing.bom_byproducts_audit_timestamp() CASCADE;
