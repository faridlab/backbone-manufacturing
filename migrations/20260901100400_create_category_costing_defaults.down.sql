-- Reverse the costing defaults create.
DROP TRIGGER IF EXISTS category_costing_defaults_update_audit ON manufacturing.category_costing_defaults;
DROP TRIGGER IF EXISTS category_costing_defaults_insert_audit ON manufacturing.category_costing_defaults;
DROP FUNCTION IF EXISTS manufacturing.category_costing_defaults_audit_timestamp();
DROP TABLE IF EXISTS manufacturing.category_costing_defaults;
